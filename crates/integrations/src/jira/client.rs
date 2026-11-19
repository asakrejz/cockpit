//! Reading client for Jira Cloud (REST v3).

use cockpit_core::Item;
use cockpit_i18n::msg;

use serde::de::DeserializeOwned;

use super::dto::{
    AssignableUser, CommentPage, JiraComment, Myself, Project, ProjectPage, SearchResult,
    TransitionList,
};
use super::map;
use crate::http::{base_url, client, send};
use crate::outbound::{Outbound, Request, choose_transition, transition_request};
use crate::{Error, Secret};
use cockpit_core::TicketStatus;

/// Issues assigned to me or watched by me that are not done yet.
pub const ASSIGNED_OR_WATCHED_JQL: &str = "(assignee = currentUser() OR watcher = currentUser()) \
     AND statusCategory != Done ORDER BY updated DESC";

/// The part of the default queries that restricts to the active sprints.
pub const SPRINT_CLAUSE: &str = "sprint in openSprints()";
/// What the fallback puts in its place: all open tickets.
const OPEN_CLAUSE: &str = "statusCategory != Done";
const ORDER: &str = "ORDER BY updated DESC";

/// The query for the sync: a custom JQL stays as it is; otherwise the tickets of the active
/// sprints – restricted to `projects` if given (all projects the key sees otherwise). Project
/// keys are checked (letters, digits, `_`), so that nothing slips into the query.
pub fn jql_for(projects: &[String], own: Option<&str>) -> Result<String, Error> {
    if let Some(j) = own.map(str::trim).filter(|j| !j.is_empty()) {
        return Ok(j.to_string());
    }
    if projects.is_empty() {
        return Ok(format!("{SPRINT_CLAUSE} {ORDER}"));
    }
    for p in projects {
        if !cockpit_core::is_project_key(p) {
            return Err(Error::Coded(msg!(
                IntegrationsJiraBadProjectKey,
                project = p
            )));
        }
    }
    Ok(format!(
        "{SPRINT_CLAUSE} AND project in ({}) {ORDER}",
        projects.join(", ")
    ))
}

/// The tickets of the active sprint of a project. Applies when a project is chosen in the app.
pub fn jql_for_project(project: &str) -> Result<String, Error> {
    if !cockpit_core::is_project_key(project) {
        return Err(Error::Coded(msg!(
            IntegrationsJiraBadProjectKey,
            project = project
        )));
    }
    Ok(format!("project = {project} AND {SPRINT_CLAUSE} {ORDER}"))
}

/// The query the sync runs and how its tickets are labelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JiraQuery {
    pub jql: String,
    /// The query was chosen in the app (project or sprint view): tickets that are neither
    /// assigned to me nor watched by me then show up with the reason „Im Projekt“.
    pub in_selection: bool,
}

/// The query for the sync, from what the app has chosen.
///
/// * Nothing chosen (`chosen` is `None` and the sprint view is „Aktueller Sprint“): as before,
///   [`jql_for`] with the settings file.
/// * Project chosen, current sprint: [`jql_for_project`].
/// * Another sprint view („Alle offenen“, „Zuletzt aktualisiert“, „Backlog“): all open tickets
///   of the project (or of the projects of the settings file; all projects if there are none) –
///   whoever they are assigned to. A JQL set by hand is only replaced when a project is chosen.
///
/// Only ever a read query; nothing here writes to Jira.
pub fn jql_for_selection(
    projects: &[String],
    own: Option<&str>,
    chosen: Option<&str>,
    sprint: cockpit_core::SprintView,
) -> Result<JiraQuery, Error> {
    use cockpit_core::SprintView;
    let plain = |jql: String, in_selection| JiraQuery { jql, in_selection };
    if sprint == SprintView::Current {
        return match chosen {
            Some(p) => jql_for_project(p).map(|j| plain(j, true)),
            None => jql_for(projects, own).map(|j| plain(j, false)),
        };
    }
    if chosen.is_none() && own.map(str::trim).is_some_and(|j| !j.is_empty()) {
        return jql_for(projects, own).map(|j| plain(j, false));
    }
    let keys = chosen.map(|p| vec![p.to_string()]);
    let keys = keys.as_deref().unwrap_or(projects);
    for p in keys {
        if !cockpit_core::is_project_key(p) {
            return Err(Error::Coded(msg!(
                IntegrationsJiraBadProjectKey,
                project = p
            )));
        }
    }
    let mut clauses = Vec::new();
    if !keys.is_empty() {
        clauses.push(format!("project in ({})", keys.join(", ")));
    }
    clauses.push(OPEN_CLAUSE.to_string());
    match sprint {
        SprintView::Backlog => clauses.push("sprint is EMPTY".to_string()),
        SprintView::Recent => {
            clauses.push(format!("updated >= -{}d", cockpit_core::RECENT_DAYS));
        }
        SprintView::AllOpen | SprintView::Current => {}
    }
    Ok(plain(format!("{} {ORDER}", clauses.join(" AND ")), true))
}

/// Is this one of the two queries [`jql_for`] and [`jql_for_project`] build by default?
pub fn is_sprint_query(jql: &str) -> bool {
    let jql = jql.trim();
    jql.ends_with(ORDER)
        && (jql.starts_with(&format!("{SPRINT_CLAUSE} "))
            || jql.contains(&format!(" AND {SPRINT_CLAUSE} {ORDER}")))
}

/// The query for projects without sprints: the same default query with all open tickets
/// instead of the active sprints. `None` for any other query – a JQL set by hand is never
/// rewritten.
pub fn open_tickets_jql(jql: &str) -> Option<String> {
    is_sprint_query(jql).then(|| jql.trim().replacen(SPRINT_CLAUSE, OPEN_CLAUSE, 1))
}

/// At most this many people come back from the search for assignable users.
pub const ASSIGNABLE_MAX: usize = 20;

/// This many projects per page and at most in total.
const PROJECTS_PAGE: usize = 50;
const MAX_PROJECTS: usize = 500;

/// All fields (the detail view shows custom fields too), except the big ones the cockpit
/// loads in other calls.
const FIELDS: &str = "*all,-comment,-worklog,-votes";

/// This many issues per page and at most in total.
const PAGE: usize = 50;
pub const MAX_ISSUES: usize = 200;

/// The issues of a search and whether more matched than were fetched.
#[derive(Debug, Default)]
pub struct SearchPage {
    pub issues: Vec<super::dto::Issue>,
    /// There were more issues than [`MAX_ISSUES`].
    pub truncated: bool,
}

pub struct JiraClient {
    http: reqwest::Client,
    base: String,
    email: String,
    token: Secret,
    /// With OAuth: `Authorization: Bearer`, otherwise Basic with email and API token.
    oauth: bool,
    /// The sprint field of the site, asked once per client (`None` inside: the site has none).
    sprint_field: std::sync::Mutex<Option<Option<String>>>,
    /// The address of the site people open in the browser (`https://acme.atlassian.net`), for
    /// links to tickets. `None` = not known.
    site: Option<String>,
}

/// A downloaded attachment.
#[derive(Debug, Clone)]
pub struct Download {
    pub bytes: Vec<u8>,
    /// The `Content-Type` Jira sent, if any.
    pub content_type: Option<String>,
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
            sprint_field: Default::default(),
            site: crate::http::base_url(site_url).ok(),
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
            sprint_field: Default::default(),
            site: None,
        })
    }

    /// Tells the client the address of the site people open in the browser (with OAuth, the
    /// base is the gateway, which no one opens). Without an `http(s)` address no links exist.
    pub fn with_site(mut self, site: Option<&str>) -> Self {
        self.site = site.and_then(|s| crate::http::base_url(s).ok());
        self
    }

    /// The page of a ticket in Jira (`<site>/browse/<KEY>`); `None` if the site is not known.
    pub fn browse_url(&self, key: &str) -> Option<String> {
        cockpit_core::details::browse_url(self.site.as_deref()?, key)
    }

    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.oauth {
            req.bearer_auth(self.token.expose())
        } else {
            req.basic_auth(&self.email, Some(self.token.expose()))
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let req = self.http.request(method, format!("{}{path}", self.base));
        self.authed(req).header("Accept", "application/json")
    }

    /// Reads an attachment (`thumbnail`: its small picture). Read only. `id` must be digits,
    /// since it goes into the address. At most `max` bytes: a larger one is [`Error::TooLarge`].
    /// The key is only sent to the site itself – a redirect to the media service (which
    /// carries its own token in the address) goes without it.
    pub async fn attachment(
        &self,
        id: &str,
        thumbnail: bool,
        max: usize,
    ) -> Result<Download, Error> {
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
            return Err(Error::NotFound);
        }
        let kind = if thumbnail { "thumbnail" } else { "content" };
        let req = self
            .http
            .get(format!("{}/rest/api/3/attachment/{kind}/{id}", self.base))
            .header("Accept", "*/*");
        let mut resp = self.authed(req).send().await.map_err(|e| {
            Error::Network(e.without_url().to_string()).without_key(self.token.expose())
        })?;
        let status = resp.status();
        if !status.is_success() {
            let headers = resp.headers().clone();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::http::error_from(status, &headers, &body, 0)
                .without_key(self.token.expose()));
        }
        if resp.content_length().is_some_and(|n| n > max as u64) {
            return Err(Error::TooLarge);
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))?
        {
            if bytes.len() + chunk.len() > max {
                return Err(Error::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Download {
            bytes,
            content_type,
        })
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
        self.search_with_sprint(jql, None).await
    }

    /// The ID of the sprint field of this site (`customfield_10020` and the like), from
    /// `GET /rest/api/3/field`: the field of the Jira Software sprint type, otherwise the one
    /// named „Sprint“. `None` if the site has none (no Jira Software). Asked once per client (the
    /// server builds one per sync); a failed question is not kept.
    pub async fn sprint_field(&self) -> Result<Option<String>, Error> {
        if let Some(known) = self.sprint_field.lock().ok().and_then(|k| k.clone()) {
            return Ok(known);
        }
        let fields: Vec<super::dto::FieldInfo> =
            self.lies_json(self.get("/rest/api/3/field")).await?;
        let is_sprint_type = |f: &super::dto::FieldInfo| {
            f.schema
                .as_ref()
                .and_then(|s| s.custom.as_deref())
                .is_some_and(|c| c.ends_with(":gh-sprint"))
        };
        let found = fields
            .iter()
            .find(|f| is_sprint_type(f))
            .or_else(|| fields.iter().find(|f| f.name == "Sprint"))
            .map(|f| f.id.clone())
            .filter(|id| !id.is_empty());
        if let Ok(mut k) = self.sprint_field.lock() {
            *k = Some(found.clone());
        }
        Ok(found)
    }

    /// Like [`JiraClient::search`], additionally asking for the sprint field.
    pub async fn search_with_sprint(
        &self,
        jql: &str,
        sprint_field: Option<&str>,
    ) -> Result<Vec<super::dto::Issue>, Error> {
        Ok(self.search_page(jql, sprint_field).await?.issues)
    }

    /// Like [`JiraClient::search_with_sprint`], and says if the result was cut at
    /// [`MAX_ISSUES`] (Jira had a further page).
    pub async fn search_page(
        &self,
        jql: &str,
        sprint_field: Option<&str>,
    ) -> Result<SearchPage, Error> {
        // `*all` already holds the sprint field; naming it too keeps the question explicit.
        let fields = match sprint_field {
            Some(f) => format!("{FIELDS},{f}"),
            None => FIELDS.to_string(),
        };
        let mut all = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut question = vec![
                ("jql", jql.to_string()),
                ("fields", fields.clone()),
                ("expand", "names".to_string()),
                ("maxResults", PAGE.to_string()),
            ];
            if let Some(t) = &token {
                question.push(("nextPageToken", t.clone()));
            }
            let res: SearchResult = self
                .lies_json(self.get("/rest/api/3/search/jql").query(&question))
                .await?;
            let names = std::sync::Arc::new(res.names);
            all.extend(res.issues.into_iter().map(|mut i| {
                i.names = names.clone();
                i
            }));
            token = res.next_page_token.filter(|t| !t.is_empty());
            if token.is_none() || all.len() >= MAX_ISSUES {
                break;
            }
        }
        let truncated = token.is_some() || all.len() > MAX_ISSUES;
        all.truncate(MAX_ISSUES);
        Ok(SearchPage {
            issues: all,
            truncated,
        })
    }

    /// The projects the key may see, sorted by name (page by page, at most
    /// [`MAX_PROJECTS`]).
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

    /// People who can be assigned to tickets of a project and whose name matches `query`
    /// (`GET /rest/api/3/user/assignable/search`, read only). Inactive accounts and apps are
    /// left out; at most [`ASSIGNABLE_MAX`] come back.
    pub async fn assignable_users(
        &self,
        project: &str,
        query: &str,
    ) -> Result<Vec<AssignableUser>, Error> {
        if !cockpit_core::is_project_key(project) {
            return Err(Error::Coded(msg!(
                IntegrationsJiraBadProjectKey,
                project = project
            )));
        }
        let question = [
            ("project", project.to_string()),
            ("query", query.trim().to_string()),
            ("maxResults", ASSIGNABLE_MAX.to_string()),
        ];
        let users: Vec<AssignableUser> = self
            .lies_json(
                self.get("/rest/api/3/user/assignable/search")
                    .query(&question),
            )
            .await?;
        Ok(users
            .into_iter()
            .filter(|u| {
                u.active
                    && !u.display_name.trim().is_empty()
                    && u.account_type.as_deref().is_none_or(|t| t == "atlassian")
            })
            .collect())
    }

    /// [`Self::assignable_users`] as people for the app: you come as [`cockpit_core::Person::me`],
    /// the others with initials from their name.
    pub async fn assignable_people(
        &self,
        project: &str,
        query: &str,
    ) -> Result<Vec<cockpit_core::Person>, Error> {
        let users = self.assignable_users(project, query).await?;
        // Without our own account ID everybody is „someone else“; that is no reason to fail.
        let me = self.me().await.ok();
        Ok(users
            .into_iter()
            .map(|u| {
                if me.as_deref() == Some(u.account_id.as_str()) {
                    cockpit_core::Person::me()
                } else {
                    crate::text::person(&u.display_name)
                }
            })
            .collect())
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
                None => Err(Error::Config("not a Jira call".into())),
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
            Error::Coded(msg!(IntegrationsJiraNoTransition, key = key, name = name))
        })?;
        self.send(&transition_request(key, &transition.id))
            .await
            .map(|_| ())
    }
}
