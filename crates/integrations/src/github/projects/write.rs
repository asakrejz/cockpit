//! Writing to GitHub Projects and issues – only ever called for an action the user clicked.
//!
//! * GraphQL: project fields (`updateProjectV2ItemFieldValue`), drafts, adding an item, converting a
//!   draft, creating and linking a project, the status field.
//! * REST: everything that belongs to the issue itself (assignees, labels, comments, state, title and
//!   body) – these need the permission **Issues**, the GraphQL ones **Projects**.
//!
//! A refusal is reported as the missing right ([`Error::MissingRight`]); nothing here deletes.

use cockpit_core::project::DEFAULT_COLUMNS;
use cockpit_core::{
    CreatedIssue, FieldInput, ProjectOrigin, ProjectRef, ProjectSummary, Right, SelectOption,
    TicketStatus,
};
use serde_json::{Value, json};

use super::queries;
use crate::Error;
use crate::github::client::GitHubClient;

/// Who an issue is assigned to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assignee {
    /// The signed-in user (resolved by `GET /user` when the call runs).
    Me,
    Login(String),
    /// Nobody: the assignees are emptied.
    Nobody,
}

/// What a REST write changes at an issue.
#[derive(Clone, Debug, PartialEq)]
pub enum IssueWrite {
    Comment(String),
    State { closed: bool },
    Edit { title: String, body: String },
    Assign(Assignee),
    Label { label: String, on: bool },
}

/// The value JSON of a field for `updateProjectV2ItemFieldValue`.
pub fn field_value_json(input: &FieldInput) -> Value {
    match input {
        FieldInput::Select(id) => json!({ "singleSelectOptionId": id }),
        FieldInput::Iteration(id) => json!({ "iterationId": id }),
        FieldInput::Text(t) => json!({ "text": t.trim() }),
        FieldInput::Number(n) => json!({ "number": n.trim().parse::<f64>().unwrap_or(0.0) }),
        FieldInput::Date(d) => json!({ "date": d.trim() }),
    }
}

/// `{ "name", "color", "description" }` for the default columns, in order.
pub fn default_status_options() -> Vec<Value> {
    let color = |s: TicketStatus| match s {
        TicketStatus::Open => "GRAY",
        TicketStatus::InProgress => "YELLOW",
        TicketStatus::Review => "ORANGE",
        TicketStatus::Done => "GREEN",
    };
    DEFAULT_COLUMNS
        .iter()
        .map(|(name, s)| json!({ "name": name, "color": color(*s), "description": "" }))
        .collect()
}

/// Percent-encodes one path segment (a label name may hold spaces, `/`, `&` …).
pub fn path_segment(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

impl GitHubClient {
    async fn mutate(&self, query: &str, vars: Value, need: Right) -> Result<Value, Error> {
        self.graphql(query, vars, need, false).await
    }

    /// REST write. A refusal means the permission "Issues" (write) is missing.
    async fn rest_write(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<String, Error> {
        let mut req = self.request(method, path);
        if let Some(b) = body {
            req = req.json(&b);
        }
        match self.text(req).await {
            Err(Error::NoPermission) => Err(Error::MissingRight {
                right: Right::IssuesWrite,
                kind: self.access().kind,
            }),
            other => other,
        }
    }

    /// Changes an issue (REST).
    pub async fn issue_write(&self, repo: &str, number: u32, w: &IssueWrite) -> Result<(), Error> {
        if !cockpit_core::repo_name_valid(repo) {
            return Err(Error::Config(format!("not a repo: {repo}")));
        }
        let base = format!("/repos/{repo}/issues/{number}");
        use reqwest::Method;
        match w {
            IssueWrite::Comment(text) => {
                self.rest_write(
                    Method::POST,
                    &format!("{base}/comments"),
                    Some(json!({ "body": text })),
                )
                .await?;
            }
            IssueWrite::State { closed: true } => {
                self.rest_write(
                    Method::PATCH,
                    &base,
                    Some(json!({ "state": "closed", "state_reason": "completed" })),
                )
                .await?;
            }
            IssueWrite::State { closed: false } => {
                self.rest_write(Method::PATCH, &base, Some(json!({ "state": "open" })))
                    .await?;
            }
            IssueWrite::Edit { title, body } => {
                self.rest_write(
                    Method::PATCH,
                    &base,
                    Some(json!({ "title": title, "body": body })),
                )
                .await?;
            }
            IssueWrite::Assign(who) => {
                let list: Vec<String> = match who {
                    Assignee::Me => vec![self.me().await?],
                    Assignee::Login(l) => vec![l.clone()],
                    Assignee::Nobody => Vec::new(),
                };
                self.rest_write(Method::PATCH, &base, Some(json!({ "assignees": list })))
                    .await?;
            }
            IssueWrite::Label { label, on: true } => {
                self.rest_write(
                    Method::POST,
                    &format!("{base}/labels"),
                    Some(json!({ "labels": [label] })),
                )
                .await?;
            }
            IssueWrite::Label { label, on: false } => {
                match self
                    .rest_write(
                        Method::DELETE,
                        &format!("{base}/labels/{}", path_segment(label)),
                        None,
                    )
                    .await
                {
                    // The label is already gone: that is what was wanted.
                    Ok(_) | Err(Error::NotFound) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    /// Sets (or clears) a field of an item.
    pub async fn set_field(
        &self,
        project_id: &str,
        item_id: &str,
        field_id: &str,
        value: Option<&FieldInput>,
    ) -> Result<(), Error> {
        match value {
            Some(v) => {
                self.mutate(
                    queries::SET_FIELD,
                    json!({ "project": project_id, "item": item_id, "field": field_id,
                            "value": field_value_json(v) }),
                    Right::ProjectsWrite,
                )
                .await?;
            }
            None => {
                self.mutate(
                    queries::CLEAR_FIELD,
                    json!({ "project": project_id, "item": item_id, "field": field_id }),
                    Right::ProjectsWrite,
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Changes title and body of a draft issue.
    pub async fn update_draft(&self, draft_id: &str, title: &str, body: &str) -> Result<(), Error> {
        self.mutate(
            queries::UPDATE_DRAFT,
            json!({ "draft": draft_id, "title": title, "body": body }),
            Right::ProjectsWrite,
        )
        .await?;
        Ok(())
    }

    /// The node ID of a repo.
    async fn repo_id(&self, repo: &str) -> Result<String, Error> {
        let (owner, name) = repo
            .split_once('/')
            .filter(|_| cockpit_core::repo_name_valid(repo))
            .ok_or_else(|| Error::Config(format!("not a repo: {repo}")))?;
        let data = self
            .graphql(
                queries::REPO_ID,
                json!({ "owner": owner, "name": name }),
                Right::IssuesRead,
                false,
            )
            .await?;
        data["repository"]["id"]
            .as_str()
            .map(str::to_string)
            .ok_or(Error::NotFound)
    }

    /// Turns a draft item into an issue of `repo`.
    pub async fn convert_draft(&self, item_id: &str, repo: &str) -> Result<CreatedIssue, Error> {
        let repo_id = self.repo_id(repo).await?;
        let data = self
            .mutate(
                queries::CONVERT_DRAFT,
                json!({ "item": item_id, "repo": repo_id }),
                Right::ProjectsWrite,
            )
            .await?;
        let item = &data["convertProjectV2DraftIssueItemToIssue"]["item"];
        let content = &item["content"];
        Ok(CreatedIssue {
            repo: content["repository"]["nameWithOwner"]
                .as_str()
                .unwrap_or(repo)
                .to_string(),
            number: content["number"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| Error::Response("no issue number in the answer".into()))?,
            content_id: content["id"].as_str().unwrap_or_default().to_string(),
            item_id: item["id"].as_str().unwrap_or(item_id).to_string(),
            url: content["url"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// Creates an issue (REST), adds it to the project (GraphQL) and, if given, sets its status
    /// (`(field ID, option ID)`). Two calls at GitHub: if the second fails the issue exists and the
    /// error says which right the project step needed.
    pub async fn create_issue_in_project(
        &self,
        project_id: &str,
        repo: &str,
        title: &str,
        body: &str,
        status: Option<(&str, &str)>,
    ) -> Result<CreatedIssue, Error> {
        if !cockpit_core::repo_name_valid(repo) {
            return Err(Error::Config(format!("not a repo: {repo}")));
        }
        let text = self
            .rest_write(
                reqwest::Method::POST,
                &format!("/repos/{repo}/issues"),
                Some(json!({ "title": title, "body": body })),
            )
            .await?;
        let issue: Value =
            serde_json::from_str(&text).map_err(|e| Error::Response(e.to_string()))?;
        let number = issue["number"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| Error::Response("no issue number in the answer".into()))?;
        let content_id = issue["node_id"].as_str().unwrap_or_default().to_string();
        let data = self
            .mutate(
                queries::ADD_ITEM,
                json!({ "project": project_id, "content": content_id }),
                Right::ProjectsWrite,
            )
            .await?;
        let item_id = data["addProjectV2ItemById"]["item"]["id"]
            .as_str()
            .ok_or_else(|| Error::Response("no item in the answer".into()))?
            .to_string();
        if let Some((field, option)) = status {
            self.set_field(
                project_id,
                &item_id,
                field,
                Some(&FieldInput::Select(option.to_string())),
            )
            .await?;
        }
        Ok(CreatedIssue {
            repo: repo.to_string(),
            number,
            content_id,
            item_id,
            url: issue["html_url"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// Creates a project for `owner` (a user or an organization), links it to `repo` if given and
    /// makes sure it has a status field with the default columns. Never deletes anything: every
    /// call is an addition.
    pub async fn create_project(
        &self,
        owner: &str,
        title: &str,
        repo: Option<&str>,
    ) -> Result<ProjectSummary, Error> {
        let data = self
            .graphql(
                queries::OWNER_ID,
                json!({ "login": owner }),
                Right::ProjectsRead,
                false,
            )
            .await?;
        let owner_id = data["repositoryOwner"]["id"]
            .as_str()
            .ok_or(Error::NotFound)?
            .to_string();
        let origin_user = self.list_owner_is_viewer(owner).await.unwrap_or(false);
        let made = self
            .mutate(
                queries::CREATE_PROJECT,
                json!({ "owner": owner_id, "title": title }),
                Right::ProjectsWrite,
            )
            .await?;
        let p = &made["createProjectV2"]["projectV2"];
        let id = p["id"]
            .as_str()
            .ok_or_else(|| Error::Response("no project in the answer".into()))?
            .to_string();
        let number = p["number"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| Error::Response("no project number in the answer".into()))?;
        let mut linked = Vec::new();
        if let Some(repo) = repo {
            let repo_id = self.repo_id(repo).await?;
            self.mutate(
                queries::LINK_PROJECT,
                json!({ "project": id, "repo": repo_id }),
                Right::ProjectsWrite,
            )
            .await?;
            linked.push(repo.to_string());
        }
        self.ensure_status_field(&ProjectRef::new(owner, number), &id)
            .await?;
        Ok(ProjectSummary {
            id,
            owner: p["owner"]["login"].as_str().unwrap_or(owner).to_string(),
            number,
            title: p["title"].as_str().unwrap_or(title).to_string(),
            url: p["url"].as_str().unwrap_or_default().to_string(),
            closed: false,
            origin: if origin_user {
                ProjectOrigin::User
            } else if linked.is_empty() {
                ProjectOrigin::Org
            } else {
                ProjectOrigin::Repo
            },
            repos: linked,
        })
    }

    /// Is `owner` the signed-in user (and not an organization)?
    async fn list_owner_is_viewer(&self, owner: &str) -> Result<bool, Error> {
        Ok(self.me().await?.eq_ignore_ascii_case(owner))
    }

    /// Makes sure the project has a single-select field `Status` with Todo, In Progress, In Review
    /// and Done: creates it if it is missing, replaces the choices if they differ (only meant for a
    /// project that was just created – replacing choices gives them new IDs).
    pub async fn ensure_status_field(
        &self,
        project: &ProjectRef,
        project_id: &str,
    ) -> Result<(), Error> {
        let meta = self.project_meta(project, None).await?;
        let want: Vec<&str> = DEFAULT_COLUMNS.iter().map(|(n, _)| *n).collect();
        match meta.status_def() {
            None => {
                self.mutate(
                    queries::CREATE_STATUS_FIELD,
                    json!({ "project": project_id, "options": default_status_options() }),
                    Right::ProjectsWrite,
                )
                .await?;
            }
            Some(def) => {
                let have: Vec<&str> = def
                    .options
                    .iter()
                    .map(|o: &SelectOption| o.name.as_str())
                    .collect();
                if have != want {
                    self.mutate(
                        queries::UPDATE_STATUS_FIELD,
                        json!({ "field": def.id, "options": default_status_options() }),
                        Right::ProjectsWrite,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }
}
