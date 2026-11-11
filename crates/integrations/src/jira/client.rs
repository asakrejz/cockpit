//! Lesender Client für Jira Cloud (REST v3).

use cockpit_core::Item;

use super::dto::{CommentPage, JiraComment, Myself, SearchResult};
use super::map;
use crate::http::{basis_url, client, lies_json};
use crate::{Fehler, Secret};

/// Mir zugewiesene oder von mir beobachtete Issues, die noch nicht erledigt sind.
pub const ASSIGNED_OR_WATCHED_JQL: &str = "(assignee = currentUser() OR watcher = currentUser()) \
     AND statusCategory != Done ORDER BY updated DESC";

const FELDER: &str = "summary,description,status,assignee,reporter,updated";
const MAX_ISSUES: &str = "50";

pub struct JiraClient {
    http: reqwest::Client,
    basis: String,
    email: String,
    token: Secret,
}

impl std::fmt::Debug for JiraClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JiraClient")
            .field("basis", &self.basis)
            .field("email", &self.email)
            .field("token", &self.token)
            .finish()
    }
}

impl JiraClient {
    /// `site_url` ist z. B. `https://firma.atlassian.net`; Anmeldung mit E-Mail und API-Token.
    pub fn new(site_url: &str, email: &str, token: Secret) -> Result<Self, Fehler> {
        Ok(Self {
            http: client()?,
            basis: basis_url(site_url)?,
            email: email.to_string(),
            token,
        })
    }

    fn get(&self, pfad: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("{}{pfad}", self.basis))
            .basic_auth(&self.email, Some(self.token.expose()))
            .header("Accept", "application/json")
    }

    /// `accountId` des Schlüsselinhabers.
    pub async fn ich(&self) -> Result<String, Fehler> {
        Ok(lies_json::<Myself>(self.get("/rest/api/3/myself"))
            .await?
            .account_id)
    }

    /// Issues zu einer JQL-Abfrage (erste Seite, höchstens 50).
    pub async fn suche(&self, jql: &str) -> Result<Vec<super::dto::Issue>, Fehler> {
        let res: SearchResult = lies_json(self.get("/rest/api/3/search/jql").query(&[
            ("jql", jql),
            ("fields", FELDER),
            ("maxResults", MAX_ISSUES),
        ]))
        .await?;
        Ok(res.issues)
    }

    pub async fn kommentare(&self, key: &str) -> Result<Vec<JiraComment>, Fehler> {
        let page: CommentPage = lies_json(
            self.get(&format!("/rest/api/3/issue/{key}/comment"))
                .query(&[("orderBy", "created")]),
        )
        .await?;
        Ok(page.comments)
    }

    /// Inbox-Einträge: mir zugewiesene oder beobachtete, offene Issues.
    pub async fn lade_inbox(&self, jetzt: i64) -> Result<Vec<Item>, Fehler> {
        let ich = self.ich().await?;
        let issues = self.suche(ASSIGNED_OR_WATCHED_JQL).await?;
        Ok(issues
            .iter()
            .map(|i| map::issue_to_item(i, &ich, jetzt))
            .collect())
    }
}
