//! Reading client for Jira Cloud (REST v3).

use cockpit_core::Item;

use serde::de::DeserializeOwned;

use super::dto::{
    CommentPage, JiraComment, Myself, Project, ProjectPage, SearchResult, TransitionList,
};
use super::map;
use crate::http::{base_url, client, send};
use crate::outbound::{Outbound, Request, choose_transition, transition_request};
use crate::{Error, Secret};
use cockpit_core::TicketStatus;

/// Issues assigned to me or watched by me that are not done yet.
pub const ASSIGNED_OR_WATCHED_JQL: &str = "(assignee = currentUser() OR watcher = currentUser()) \
     AND statusCategory != Done ORDER BY updated DESC";

/// The query for the sync: a custom JQL, otherwise open issues assigned to me or
/// watched by me – restricted to `projects` if given. Project keys
/// are checked (letters, digits, `_`), so that nothing slips into the query.
pub fn jql_for(projects: &[String], own: Option<&str>) -> Result<String, Error> {
    if let Some(j) = own.map(str::trim).filter(|j| !j.is_empty()) {
        return Ok(j.to_string());
    }
    if projects.is_empty() {
        return Ok(ASSIGNED_OR_WATCHED_JQL.to_string());
    }
    for p in projects {
        if !cockpit_core::is_project_key(p) {
            return Err(Error::Config(format!("Ungültiger Projektschlüssel: {p}")));
        }
    }
    Ok(format!(
        "project in ({}) AND (assignee = currentUser() OR watcher = currentUser()) \
         AND statusCategory != Done ORDER BY updated DESC",
        projects.join(", ")
    ))
}

/// All open tickets of a project – regardless of who owns them. Applies when a
/// project is chosen in the app.
pub fn jql_for_project(project: &str) -> Result<String, Error> {
    if !cockpit_core::is_project_key(project) {
        return Err(Error::Config(format!(
            "Ungültiger Projektschlüssel: {project}"
        )));
    }
    Ok(format!(
        "project = {project} AND statusCategory != Done ORDER BY updated DESC"
    ))
}

/// This many projects per page and at most in total.
const PROJECTS_PAGE: usize = 50;
const MAX_PROJECTS: usize = 500;

const FIELDS: &str = "summary,description,status,assignee,reporter,updated,duedate,labels";
/// This many issues per page and at most in total.
const PAGE: usize = 50;
const MAX_ISSUES: usize = 150;

pub struct JiraClient {
    http: reqwest::Client,
    base: String,
    email: String,
    token: Secret,
    /// With OAuth: `Authorization: Bearer`, otherwise Basic with email and API token.
    oauth: bool,
}

impl std::fmt::Debug for JiraClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JiraClient")
            .field("basis", &self.base)
            .field("email", &self.email)
            .field("token", &self.token)
            .finish()
    }
}

impl JiraClient {
    /// `site_url` is e.g. `https://firma.atlassian.net`; sign-in with email and API token.
    pub fn new(site_url: &str, email: &str, token: Secret) -> Result<Self, Error> {
        Ok(Self {
            http: client()?,
            base: base_url(site_url)?,
            email: email.to_string(),
            token,
            oauth: false,
        })
    }

    /// Sign-in via OAuth: `base` is `https://api.atlassian.com/ex/jira/<cloudId>`
    /// ([`crate::oauth::jira_base`]), `access` the access token.
    pub fn oauth(base: &str, access: Secret) -> Result<Self, Error> {
        Ok(Self {
            http: client()?,
            base: base_url(base)?,
            email: String::new(),
            token: access,
            oauth: true,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let req = self.http.request(method, format!("{}{path}", self.base));
        let req = if self.oauth {
            req.bearer_auth(self.token.expose())
        } else {
            req.basic_auth(&self.email, Some(self.token.expose()))
        };
        req.header("Accept", "application/json")
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, path)
    }

    /// Sends the request; a key that the service echoes back in a message
    /// is made unrecognizable.
    async fn text(&self, req: reqwest::RequestBuilder) -> Result<String, Error> {
        send(req)
            .await
            .map_err(|e| e.without_key(self.token.expose()))
    }

    async fn lies_json<T: DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        let text = self.text(req).await?;
        serde_json::from_str(&text).map_err(|e| Error::Response(e.to_string()))
    }

    async fn send(&self, a: &Request) -> Result<String, Error> {
        let method = reqwest::Method::from_bytes(a.method.as_bytes())
            .map_err(|e| Error::Config(e.to_string()))?;
        let mut req = self.request(method, &a.path);
        if let Some(body) = &a.body {
            req = req.json(body);
        }
        self.text(req).await
    }

    /// `accountId` of the key holder.
    pub async fn me(&self) -> Result<String, Error> {
        Ok(self
            .lies_json::<Myself>(self.get("/rest/api/3/myself"))
            .await?
            .account_id)
    }

    /// The display name of the key holder (empty if Jira names none). Read only; serves the
    /// connection test in the setup assistant.
    pub async fn my_name(&self) -> Result<String, Error> {
        Ok(self
            .lies_json::<Myself>(self.get("/rest/api/3/myself"))
            .await?
            .display_name
            .unwrap_or_default())
    }

    /// Issues for a JQL query, page by page (`nextPageToken`), at most [`MAX_ISSUES`].
    pub async fn search(&self, jql: &str) -> Result<Vec<super::dto::Issue>, Error> {
        let mut all = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut question = vec![
                ("jql", jql.to_string()),
                ("fields", FIELDS.to_string()),
                ("maxResults", PAGE.to_string()),
            ];
            if let Some(t) = &token {
                question.push(("nextPageToken", t.clone()));
            }
            let res: SearchResult = self
                .lies_json(self.get("/rest/api/3/search/jql").query(&question))
                .await?;
            all.extend(res.issues);
            token = res.next_page_token.filter(|t| !t.is_empty());
            if token.is_none() || all.len() >= MAX_ISSUES {
                break;
            }
        }
        all.truncate(MAX_ISSUES);
        Ok(all)
    }

    /// The projects the key may see, sorted by name (page by page, at most
    /// [`MAX_PROJEKTE`]).
    pub async fn projects(&self) -> Result<Vec<Project>, Error> {
        let mut all = Vec::new();
        loop {
            let question = [
                ("startAt", all.len().to_string()),
                ("maxResults", PROJECTS_PAGE.to_string()),
                ("orderBy", "name".to_string()),
            ];
            let page: ProjectPage = self
                .lies_json(self.get("/rest/api/3/project/search").query(&question))
                .await?;
            let empty = page.values.is_empty();
            all.extend(page.values);
            if page.is_last || empty || all.len() >= MAX_PROJECTS {
                break;
            }
        }
        all.truncate(MAX_PROJECTS);
        Ok(all)
    }

    pub async fn comments(&self, key: &str) -> Result<Vec<JiraComment>, Error> {
        let page: CommentPage = self
            .lies_json(
                self.get(&format!("/rest/api/3/issue/{key}/comment"))
                    .query(&[("orderBy", "created")]),
            )
            .await?;
        Ok(page.comments)
    }

    /// Inbox entries: open issues assigned to me or watched.
    pub async fn load_inbox(&self, now: i64) -> Result<Vec<Item>, Error> {
        let me = self.me().await?;
        let issues = self.search(ASSIGNED_OR_WATCHED_JQL).await?;
        Ok(issues
            .iter()
            .map(|i| map::issue_to_item(i, &me, now))
            .collect())
    }
}

// ---------- Writing ----------

impl JiraClient {
    /// Executes an outbound call. Jira variants only.
    pub async fn execute(&self, a: &Outbound) -> Result<(), Error> {
        match a {
            Outbound::JiraStatus { key, status } => self.status_set(key, *status).await,
            other => match other.request() {
                Some(request) => self.send(&request).await.map(|_| ()),
                None => Err(Error::Config("Kein Jira-Aufruf".into())),
            },
        }
    }

    /// Finds the transition that leads to the target status and executes it.
    pub async fn status_set(&self, key: &str, target: TicketStatus) -> Result<(), Error> {
        let list: TransitionList = self
            .lies_json(self.get(&format!("/rest/api/3/issue/{key}/transitions")))
            .await?;
        let transition = choose_transition(&list.transitions, target).ok_or_else(|| {
            let name = TicketStatus::OPTIONS
                .iter()
                .find(|(id, _)| *id == target.id())
                .map_or("", |(_, l)| l);
            Error::NotPossible(format!(
                "Jira bietet für {key} keine Transition nach „{name}“ an"
            ))
        })?;
        self.send(&transition_request(key, &transition.id))
            .await
            .map(|_| ())
    }
}
