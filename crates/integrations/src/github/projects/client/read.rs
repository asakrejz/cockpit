//! Reading the projects of GitHub Projects (v2): the list to choose from, one project with its
//! fields, its items and the details of its issues.

use cockpit_core::{
    FieldDef, FieldKind, ProjectMeta, ProjectOrigin, ProjectRef, ProjectSummary, Right,
    SelectOption,
};
use serde_json::{Value, json};

use super::super::dto::{
    DetailNode, DetailsData, ItemNode, ItemsData, MetaData, ProjectNode, RepoProjects, ViewerData,
};
use super::super::queries;
use crate::Error;
use crate::github::client::GitHubClient;

/// Items per project that one sync loads at most.
pub const MAX_ITEMS: usize = 1500;
/// Issues per details call.
const DETAIL_CHUNK: usize = 20;
/// Repos per call when asking for linked projects.
const REPO_CHUNK: usize = 20;

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
