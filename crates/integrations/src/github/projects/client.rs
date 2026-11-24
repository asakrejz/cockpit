//! The GraphQL calls. Reading only – writes are in `projects/write.rs`.

use cockpit_core::{
    Access, FieldDef, FieldKind, ProjectMeta, ProjectOrigin, ProjectRef, ProjectSummary, Right,
    SelectOption, TokenKind,
};
use serde_json::{Value, json};

use super::dto::{
    DetailNode, DetailsData, Envelope, GqlError, ItemNode, ItemsData, MetaData, ProjectNode,
    RateLimit, RepoProjects, ViewerData,
};
use super::queries;
use crate::github::client::GitHubClient;
use crate::http::{error_from, now_s};
use crate::{Error, time};

/// Below this many points left the client stops asking until the limit resets.
const RESERVE: u32 = 15;
/// Items per project that one sync loads at most.
pub const MAX_ITEMS: usize = 1500;
/// Issues per details call.
const DETAIL_CHUNK: usize = 20;
/// Repos per call when asking for linked projects.
const REPO_CHUNK: usize = 20;

/// The GraphQL rate limit as the last answer reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rate {
    pub cost: u32,
    pub remaining: u32,
    /// When the limit resets (seconds since 1970), if the answer said.
    pub reset_s: Option<i64>,
}

/// The projects the credential can see, and the owners (the user and their organizations) a new
/// project could be created for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectList {
    pub projects: Vec<ProjectSummary>,
    pub owners: Vec<String>,
}

/// The items of a project as loaded.
#[derive(Debug, Default)]
pub struct ItemsPage {
    pub items: Vec<ItemNode>,
    /// There are more items than [`MAX_ITEMS`].
    pub truncated: bool,
}

impl GitHubClient {
    /// `https://api.github.com/graphql`; GitHub Enterprise Server (`https://host/api/v3`) has
    /// it at `https://host/api/graphql`.
    pub fn graphql_url(&self) -> String {
        match self.base.strip_suffix("/api/v3") {
            Some(host) => format!("{host}/api/graphql"),
            None => format!("{}/graphql", self.base),
        }
    }

    /// The rate limit of the last GraphQL answer.
    pub fn rate(&self) -> Option<Rate> {
        self.rate.lock().ok().and_then(|r| r.clone())
    }

    /// One GraphQL call. `need` is the right the call is about: a refusal is reported as that
    /// right missing. `lenient`: errors of type `NOT_FOUND` do not fail the call (the data
    /// then holds `null` where something is not visible).
    pub(crate) async fn graphql(
        &self,
        query: &str,
        variables: Value,
        need: Right,
        lenient: bool,
    ) -> Result<Value, Error> {
        if let Some(rate) = self.rate()
            && rate.remaining < RESERVE
            && let Some(reset) = rate.reset_s
            && reset > now_s() as i64
        {
            return Err(Error::RateLimit {
                wait_s: Some((reset - now_s() as i64) as u64),
            });
        }
        let req = self
            .http
            .post(self.graphql_url())
            .bearer_auth(self.token.expose())
            .header("Accept", "application/json")
            .json(&json!({ "query": query, "variables": variables }));
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))
            .map_err(|e| e.without_key(self.token.expose()))?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))?;
        self.learn_access(&headers);
        if !status.is_success() {
            let e = error_from(status, &headers, &text, now_s());
            return Err(match e {
                // A 403 here is a refusal of the right, not a quota (that is `RateLimit`).
                Error::NoPermission => self.missing(need),
                other => other.without_key(self.token.expose()),
            });
        }
        let env: Envelope = serde_json::from_str(&text)
            .map_err(|e| Error::Response(e.to_string()))
            .map_err(|e| e.without_key(self.token.expose()))?;
        let data = env.data.unwrap_or(Value::Null);
        if let Some(r) = data
            .get("rateLimit")
            .and_then(|v| serde_json::from_value::<RateLimit>(v.clone()).ok())
            && let Ok(mut slot) = self.rate.lock()
        {
            *slot = Some(Rate {
                cost: r.cost,
                remaining: r.remaining,
                reset_s: time::parse_iso(&r.reset_at),
            });
        }
        let blocking: Vec<&GqlError> = env
            .errors
            .iter()
            .filter(|e| !(lenient && e.kind == "NOT_FOUND" && !data.is_null()))
            .collect();
        if !blocking.is_empty() {
            return Err(self.classify(&blocking, need));
        }
        Ok(data)
    }

    /// Remembers what an answer says about the credential: the scopes of a classic token.
    fn learn_access(&self, headers: &reqwest::header::HeaderMap) {
        let Some(h) = headers.get("x-oauth-scopes").and_then(|v| v.to_str().ok()) else {
            return;
        };
        if let Ok(mut a) = self.access.lock() {
            // A GitHub App token never carries the header; one that does is a classic token.
            if a.kind != TokenKind::App {
                a.kind = TokenKind::Classic;
            }
            a.scopes = Some(cockpit_core::permissions::parse_scopes(h));
        }
    }

    fn missing(&self, right: Right) -> Error {
        Error::MissingRight {
            right,
            kind: self.access().kind,
        }
    }

    /// Turns the errors of a GraphQL answer into one [`Error`].
    fn classify(&self, errors: &[&GqlError], need: Right) -> Error {
        let kind_is = |k: &str| errors.iter().any(|e| e.kind == k);
        let says = |w: &str| {
            errors
                .iter()
                .any(|e| e.message.to_lowercase().contains(&w.to_lowercase()))
        };
        if kind_is("RATE_LIMITED") || says("rate limit") {
            let wait = self
                .rate()
                .and_then(|r| r.reset_s)
                .map(|t| (t - now_s() as i64).max(1) as u64)
                .or(Some(60));
            return Error::RateLimit { wait_s: wait };
        }
        if let Some(e) = errors.iter().find(|e| e.kind == "INSUFFICIENT_SCOPES") {
            return Error::MissingRight {
                right: right_from_scope_message(&e.message, need),
                kind: TokenKind::Classic,
            };
        }
        if let Some(e) = errors
            .iter()
            .find(|e| e.kind == "FORBIDDEN" || e.message.contains("Resource not accessible"))
        {
            // A refusal below `content` is about the issue behind the item, not the project.
            let right = if e.path.iter().any(|p| p.as_str() == Some("content")) {
                match need {
                    Right::ProjectsWrite | Right::IssuesWrite => Right::IssuesWrite,
                    _ => Right::IssuesRead,
                }
            } else {
                need
            };
            return self.missing(right);
        }
        if kind_is("NOT_FOUND") {
            return Error::NotFound;
        }
        let first = errors
            .first()
            .map(|e| e.message.clone())
            .unwrap_or_default();
        Error::Rejected(first)
    }

    /// The login and the access of the credential: one cheap REST call that also returns the
    /// scopes of a classic token in `X-OAuth-Scopes`. Read only.
    pub async fn probe_access(&self) -> Result<(String, Access), Error> {
        let resp = self
            .get("/user")
            .send()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))
            .map_err(|e| e.without_key(self.token.expose()))?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))?;
        self.learn_access(&headers);
        if !status.is_success() {
            return Err(error_from(status, &headers, &text, now_s()));
        }
        let login = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["login"].as_str().map(str::to_string))
            .unwrap_or_default();
        Ok((login, self.access()))
    }

    /// People who can be assigned to issues of `repo` (`GET /repos/{repo}/assignees`) whose login
    /// contains `query` (case-insensitive), at most 30. Read only.
    pub async fn assignees(&self, repo: &str, query: &str) -> Result<Vec<String>, Error> {
        if !cockpit_core::repo_name_valid(repo) {
            return Err(Error::Config(format!("not a repo: {repo}")));
        }
        #[derive(serde::Deserialize)]
        struct Login {
            login: String,
        }
        let people: Vec<Login> = self.pages(&format!("/repos/{repo}/assignees")).await?;
        let q = query.trim().to_lowercase();
        Ok(people
            .into_iter()
            .map(|p| p.login)
            .filter(|l| q.is_empty() || l.to_lowercase().contains(&q))
            .take(30)
            .collect())
    }

    // ---------- Reading ----------

    /// The projects the credential can see: its own, those of its organizations, and those
    /// linked to `repos` (`owner/name`). Without the right to read projects: [`Error::MissingRight`].
    pub async fn list_projects(&self, repos: &[String]) -> Result<ProjectList, Error> {
        let data = self
            .graphql(
                &queries::list_viewer(),
                json!({}),
                Right::ProjectsRead,
                true,
            )
            .await?;
        let viewer = serde_json::from_value::<ViewerData>(data)
            .map_err(|e| Error::Response(e.to_string()))?
            .viewer
            .unwrap_or_default();
        let mut owners: Vec<String> = Vec::new();
        if !viewer.login.is_empty() {
            owners.push(viewer.login.clone());
        }
        owners.extend(
            viewer
                .organizations
                .iter()
                .flat_map(|c| c.items())
                .map(|o| o.login.clone())
                .filter(|l| !l.is_empty()),
        );
        let mut out: Vec<ProjectSummary> = Vec::new();
        let mut add = |n: &ProjectNode, origin: ProjectOrigin, repo: Option<&str>| {
            let owner = n
                .owner
                .as_ref()
                .map(|o| o.login.clone())
                .unwrap_or_default();
            if n.id.is_empty() || owner.is_empty() {
                return;
            }
            match out.iter_mut().find(|p| p.id == n.id) {
                Some(p) => {
                    if let Some(r) = repo
                        && !p.repos.iter().any(|x| x == r)
                    {
                        p.repos.push(r.to_string());
                    }
                }
                None => out.push(ProjectSummary {
                    id: n.id.clone(),
                    owner,
                    number: n.number,
                    title: n.title.clone(),
                    url: n.url.clone(),
                    closed: n.closed,
                    origin,
                    repos: repo.map(str::to_string).into_iter().collect(),
                }),
            }
        };
        for n in viewer.projects_v2.iter().flat_map(|c| c.items()) {
            add(n, ProjectOrigin::User, None);
        }
        for org in viewer.organizations.iter().flat_map(|c| c.items()) {
            for n in org.projects_v2.iter().flat_map(|c| c.items()) {
                add(n, ProjectOrigin::Org, None);
            }
        }
        let valid: Vec<(&str, &str)> = repos
            .iter()
            .filter(|r| cockpit_core::repo_name_valid(r))
            .filter_map(|r| r.split_once('/'))
            .collect();
        for chunk in valid.chunks(REPO_CHUNK) {
            let mut vars = serde_json::Map::new();
            for (i, (o, n)) in chunk.iter().enumerate() {
                vars.insert(format!("o{i}"), json!(o));
                vars.insert(format!("n{i}"), json!(n));
            }
            let data = self
                .graphql(
                    &queries::list_repos(chunk.len()),
                    Value::Object(vars),
                    Right::ProjectsRead,
                    true,
                )
                .await?;
            for i in 0..chunk.len() {
                let Some(node) = data.get(format!("r{i}")).filter(|v| !v.is_null()) else {
                    continue;
                };
                let repo: RepoProjects = serde_json::from_value(node.clone())
                    .map_err(|e| Error::Response(e.to_string()))?;
                for n in repo.projects_v2.iter().flat_map(|c| c.items()) {
                    add(n, ProjectOrigin::Repo, Some(&repo.name_with_owner));
                }
            }
        }
        out.sort_by(|a, b| {
            (a.owner.to_lowercase(), a.number).cmp(&(b.owner.to_lowercase(), b.number))
        });
        Ok(ProjectList {
            projects: out,
            owners,
        })
    }

    /// One project with its fields. `status_field`: the name of the field that counts as the
    /// ticket status (default `Status`).
    pub async fn project_meta(
        &self,
        project: &ProjectRef,
        status_field: Option<&str>,
    ) -> Result<ProjectMeta, Error> {
        let data = self
            .graphql(
                &queries::project_meta(),
                json!({ "login": project.owner, "number": project.number }),
                Right::ProjectsRead,
                false,
            )
            .await?;
        let full = serde_json::from_value::<MetaData>(data)
            .map_err(|e| Error::Response(e.to_string()))?
            .repository_owner
            .and_then(|o| o.project_v2)
            .filter(|p| !p.id.is_empty())
            .ok_or(Error::NotFound)?;
        let fields = full
            .fields
            .iter()
            .flat_map(|c| c.items())
            .filter_map(|f| {
                let (id, name) = (f.id.clone()?, f.name.clone()?);
                let kind = match f.data_type.as_deref() {
                    Some("SINGLE_SELECT") => FieldKind::SingleSelect,
                    Some("ITERATION") => FieldKind::Iteration,
                    Some("NUMBER") => FieldKind::Number,
                    Some("DATE") => FieldKind::Date,
                    Some("TEXT") => FieldKind::Text,
                    _ => FieldKind::Other,
                };
                let options = match kind {
                    FieldKind::SingleSelect => f
                        .options
                        .iter()
                        .flatten()
                        .map(|o| SelectOption {
                            id: o.id.clone(),
                            name: o.name.clone(),
                        })
                        .collect(),
                    FieldKind::Iteration => f
                        .configuration
                        .iter()
                        .flat_map(|c| c.iterations.iter().chain(c.completed_iterations.iter()))
                        .map(|i| SelectOption {
                            id: i.id.clone(),
                            name: i.title.clone(),
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                Some(FieldDef {
                    id,
                    name,
                    kind,
                    options,
                })
            })
            .collect();
        Ok(ProjectMeta {
            id: full.id,
            owner: project.owner.clone(),
            number: project.number,
            title: full.title,
            url: full.url,
            fields,
            status_field: status_field.map(str::to_string),
        })
    }

    /// The items of a project, page by page (50 each), at most [`MAX_ITEMS`]; archived ones are
    /// left out. The light part only: fields, assignees, labels.
    pub async fn project_items(&self, project_id: &str) -> Result<ItemsPage, Error> {
        let mut page = ItemsPage::default();
        let mut after: Option<String> = None;
        loop {
            let data = self
                .graphql(
                    &queries::project_items(),
                    json!({ "id": project_id, "after": after }),
                    Right::ProjectsRead,
                    false,
                )
                .await?;
            let conn = serde_json::from_value::<ItemsData>(data)
                .map_err(|e| Error::Response(e.to_string()))?
                .node
                .and_then(|n| n.items)
                .ok_or(Error::NotFound)?;
            page.items
                .extend(conn.nodes.into_iter().flatten().filter(|i| !i.is_archived));
            if page.items.len() >= MAX_ITEMS {
                page.truncated = conn.page_info.has_next_page;
                page.items.truncate(MAX_ITEMS);
                break;
            }
            match (conn.page_info.has_next_page, conn.page_info.end_cursor) {
                (true, Some(c)) => after = Some(c),
                _ => break,
            }
        }
        Ok(page)
    }

    /// Comments, timeline and closing references of issues (by node ID), in chunks. The second
    /// value tells whether issue type, parent and sub-issues could be asked for (a server
    /// without them answers without).
    pub async fn item_details(&self, ids: &[String]) -> Result<(Vec<DetailNode>, bool), Error> {
        let mut out = Vec::new();
        let mut extras = true;
        for chunk in ids.chunks(DETAIL_CHUNK) {
            let vars = json!({ "ids": chunk });
            let mut answer = self
                .graphql(
                    &queries::issue_details(extras),
                    vars.clone(),
                    Right::IssuesRead,
                    false,
                )
                .await;
            if extras
                && let Err(Error::Rejected(m)) = &answer
                && m.contains("doesn't exist on type")
            {
                extras = false;
                answer = self
                    .graphql(
                        &queries::issue_details(false),
                        vars,
                        Right::IssuesRead,
                        false,
                    )
                    .await;
            }
            let nodes = serde_json::from_value::<DetailsData>(answer?)
                .map_err(|e| Error::Response(e.to_string()))?
                .nodes;
            out.extend(nodes.into_iter().flatten().filter(|n| !n.id.is_empty()));
        }
        Ok((out, extras))
    }
}

/// Which right an `INSUFFICIENT_SCOPES` message asks for: it names the scopes the field needs
/// (`… requires one of the following scopes: ['read:project'] …`).
pub(crate) fn right_from_scope_message(message: &str, need: Right) -> Right {
    let wanted = message
        .split_once("scopes: [")
        .map(|(_, rest)| rest.split(']').next().unwrap_or(""))
        .unwrap_or("");
    if wanted.contains("'read:project'") {
        Right::ProjectsRead
    } else if wanted.contains("'project'") {
        Right::ProjectsWrite
    } else if wanted.contains("'repo'") || wanted.contains("'public_repo'") {
        match need {
            Right::ProjectsWrite | Right::IssuesWrite => Right::IssuesWrite,
            _ => Right::IssuesRead,
        }
    } else {
        need
    }
}
