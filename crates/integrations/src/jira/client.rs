//! Lesender Client für Jira Cloud (REST v3).

use cockpit_core::Item;

use serde::de::DeserializeOwned;

use super::dto::{
    CommentPage, JiraComment, Myself, Project, ProjectPage, SearchResult, TransitionList,
};
use super::map;
use crate::outbound::{Request, Outbound, transition_request, choose_transition};
use crate::http::{base_url, client, send};
use crate::{Error, Secret};
use cockpit_core::TicketStatus;

/// Mir zugewiesene oder von mir beobachtete Issues, die noch nicht erledigt sind.
pub const ASSIGNED_OR_WATCHED_JQL: &str = "(assignee = currentUser() OR watcher = currentUser()) \
     AND statusCategory != Done ORDER BY updated DESC";

/// Die Abfrage für den Abgleich: eine eigene JQL, sonst mir zugewiesene oder von mir
/// beobachtete offene Issues – auf `projekte` beschränkt, falls angegeben. Projektschlüssel
/// werden geprüft (Buchstaben, Ziffern, `_`), damit nichts in die Abfrage rutscht.
pub fn jql_for(projects: &[String], own: Option<&str>) -> Result<String, Error> {
    if let Some(j) = own.map(str::trim).filter(|j| !j.is_empty()) {
        return Ok(j.to_string());
    }
    if projects.is_empty() {
        return Ok(ASSIGNED_OR_WATCHED_JQL.to_string());
    }
    for p in projects {
        if !cockpit_core::is_project_key(p) {
            return Err(Error::Config(format!(
                "Ungültiger Projektschlüssel: {p}"
            )));
        }
    }
    Ok(format!(
        "project in ({}) AND (assignee = currentUser() OR watcher = currentUser()) \
         AND statusCategory != Done ORDER BY updated DESC",
        projects.join(", ")
    ))
}

/// Alle offenen Tickets eines Projekts – egal, wem sie gehören. Gilt, wenn in der App ein
/// Projekt gewählt ist.
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

/// So viele Projekte je Seite und höchstens in Summe.
const PROJECTS_PAGE: usize = 50;
const MAX_PROJECTS: usize = 500;

const FIELDS: &str = "summary,description,status,assignee,reporter,updated,duedate,labels";
/// So viele Issues je Seite und höchstens in Summe.
const PAGE: usize = 50;
const MAX_ISSUES: usize = 150;

pub struct JiraClient {
    http: reqwest::Client,
    base: String,
    email: String,
    token: Secret,
    /// Mit OAuth: `Authorization: Bearer`, sonst Basic mit E-Mail und API-Token.
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
    /// `site_url` ist z. B. `https://firma.atlassian.net`; Anmeldung mit E-Mail und API-Token.
    pub fn new(site_url: &str, email: &str, token: Secret) -> Result<Self, Error> {
        Ok(Self {
            http: client()?,
            base: base_url(site_url)?,
            email: email.to_string(),
            token,
            oauth: false,
        })
    }

    /// Anmeldung per OAuth: `basis` ist `https://api.atlassian.com/ex/jira/<cloudId>`
    /// ([`crate::oauth::jira_basis`]), `access` das Access-Token.
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

    /// Schickt die Anfrage; ein Schlüssel, den der Dienst in einer Meldung zurückspielt,
    /// wird unkenntlich gemacht.
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

    /// `accountId` des Schlüsselinhabers.
    pub async fn me(&self) -> Result<String, Error> {
        Ok(self
            .lies_json::<Myself>(self.get("/rest/api/3/myself"))
            .await?
            .account_id)
    }

    /// Der Anzeigename des Schlüsselinhabers (leer, wenn Jira keinen nennt). Nur Lesen; dient dem
    /// Verbindungstest im Einrichtungsassistenten.
    pub async fn my_name(&self) -> Result<String, Error> {
        Ok(self
            .lies_json::<Myself>(self.get("/rest/api/3/myself"))
            .await?
            .display_name
            .unwrap_or_default())
    }

    /// Issues zu einer JQL-Abfrage, seitenweise (`nextPageToken`), höchstens [`MAX_ISSUES`].
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

    /// Die Projekte, die der Schlüssel sehen darf, nach Namen sortiert (seitenweise, höchstens
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

    /// Inbox-Einträge: mir zugewiesene oder beobachtete, offene Issues.
    pub async fn load_inbox(&self, now: i64) -> Result<Vec<Item>, Error> {
        let me = self.me().await?;
        let issues = self.search(ASSIGNED_OR_WATCHED_JQL).await?;
        Ok(issues
            .iter()
            .map(|i| map::issue_to_item(i, &me, now))
            .collect())
    }
}

// ---------- Schreiben ----------

impl JiraClient {
    /// Führt einen Aufruf nach außen aus. Nur Jira-Varianten.
    pub async fn execute(&self, a: &Outbound) -> Result<(), Error> {
        match a {
            Outbound::JiraStatus { key, status } => self.status_set(key, *status).await,
            other => match other.request() {
                Some(request) => self.send(&request).await.map(|_| ()),
                None => Err(Error::Config("Kein Jira-Aufruf".into())),
            },
        }
    }

    /// Sucht die Transition, die zum Ziel-Status führt, und führt sie aus.
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
