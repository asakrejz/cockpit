//! Lesender Client für Jira Cloud (REST v3).

use cockpit_core::Item;

use serde::de::DeserializeOwned;

use super::dto::{CommentPage, JiraComment, Myself, SearchResult, TransitionList};
use super::map;
use crate::ausgehend::{Anfrage, Ausgehend, transition_anfrage, waehle_transition};
use crate::http::{basis_url, client, sende};
use crate::{Fehler, Secret};
use cockpit_core::TicketStatus;

/// Mir zugewiesene oder von mir beobachtete Issues, die noch nicht erledigt sind.
pub const ASSIGNED_OR_WATCHED_JQL: &str = "(assignee = currentUser() OR watcher = currentUser()) \
     AND statusCategory != Done ORDER BY updated DESC";

/// Die Abfrage für den Abgleich: eine eigene JQL, sonst mir zugewiesene oder von mir
/// beobachtete offene Issues – auf `projekte` beschränkt, falls angegeben. Projektschlüssel
/// werden geprüft (Buchstaben, Ziffern, `_`), damit nichts in die Abfrage rutscht.
pub fn jql_fuer(projekte: &[String], eigene: Option<&str>) -> Result<String, Fehler> {
    if let Some(j) = eigene.map(str::trim).filter(|j| !j.is_empty()) {
        return Ok(j.to_string());
    }
    if projekte.is_empty() {
        return Ok(ASSIGNED_OR_WATCHED_JQL.to_string());
    }
    for p in projekte {
        let ok = p.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            return Err(Fehler::Einstellung(format!("Ungültiger Projektschlüssel: {p}")));
        }
    }
    Ok(format!(
        "project in ({}) AND (assignee = currentUser() OR watcher = currentUser()) \
         AND statusCategory != Done ORDER BY updated DESC",
        projekte.join(", ")
    ))
}

const FELDER: &str = "summary,description,status,assignee,reporter,updated,duedate";
/// So viele Issues je Seite und höchstens in Summe.
const SEITE: usize = 50;
const MAX_ISSUES: usize = 150;

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

    fn anfrage(&self, methode: reqwest::Method, pfad: &str) -> reqwest::RequestBuilder {
        self.http
            .request(methode, format!("{}{pfad}", self.basis))
            .basic_auth(&self.email, Some(self.token.expose()))
            .header("Accept", "application/json")
    }

    fn get(&self, pfad: &str) -> reqwest::RequestBuilder {
        self.anfrage(reqwest::Method::GET, pfad)
    }

    /// Schickt die Anfrage; ein Schlüssel, den der Dienst in einer Meldung zurückspielt,
    /// wird unkenntlich gemacht.
    async fn text(&self, req: reqwest::RequestBuilder) -> Result<String, Fehler> {
        sende(req)
            .await
            .map_err(|e| e.ohne_schluessel(self.token.expose()))
    }

    async fn lies_json<T: DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T, Fehler> {
        let text = self.text(req).await?;
        serde_json::from_str(&text).map_err(|e| Fehler::Antwort(e.to_string()))
    }

    async fn schicke(&self, a: &Anfrage) -> Result<String, Fehler> {
        let methode = reqwest::Method::from_bytes(a.methode.as_bytes())
            .map_err(|e| Fehler::Einstellung(e.to_string()))?;
        let mut req = self.anfrage(methode, &a.pfad);
        if let Some(body) = &a.body {
            req = req.json(body);
        }
        self.text(req).await
    }

    /// `accountId` des Schlüsselinhabers.
    pub async fn ich(&self) -> Result<String, Fehler> {
        Ok(self
            .lies_json::<Myself>(self.get("/rest/api/3/myself"))
            .await?
            .account_id)
    }

    /// Issues zu einer JQL-Abfrage, seitenweise (`nextPageToken`), höchstens [`MAX_ISSUES`].
    pub async fn suche(&self, jql: &str) -> Result<Vec<super::dto::Issue>, Fehler> {
        let mut alle = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut frage = vec![
                ("jql", jql.to_string()),
                ("fields", FELDER.to_string()),
                ("maxResults", SEITE.to_string()),
            ];
            if let Some(t) = &token {
                frage.push(("nextPageToken", t.clone()));
            }
            let res: SearchResult = self
                .lies_json(self.get("/rest/api/3/search/jql").query(&frage))
                .await?;
            alle.extend(res.issues);
            token = res.next_page_token.filter(|t| !t.is_empty());
            if token.is_none() || alle.len() >= MAX_ISSUES {
                break;
            }
        }
        alle.truncate(MAX_ISSUES);
        Ok(alle)
    }

    pub async fn kommentare(&self, key: &str) -> Result<Vec<JiraComment>, Fehler> {
        let page: CommentPage = self
            .lies_json(
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

// ---------- Schreiben ----------

impl JiraClient {
    /// Führt einen Aufruf nach außen aus. Nur Jira-Varianten.
    pub async fn ausfuehren(&self, a: &Ausgehend) -> Result<(), Fehler> {
        match a {
            Ausgehend::JiraStatus { key, status } => self.status_setzen(key, *status).await,
            andere => match andere.anfrage() {
                Some(anfrage) => self.schicke(&anfrage).await.map(|_| ()),
                None => Err(Fehler::Einstellung("Kein Jira-Aufruf".into())),
            },
        }
    }

    /// Sucht die Transition, die zum Ziel-Status führt, und führt sie aus.
    pub async fn status_setzen(&self, key: &str, ziel: TicketStatus) -> Result<(), Fehler> {
        let liste: TransitionList = self
            .lies_json(self.get(&format!("/rest/api/3/issue/{key}/transitions")))
            .await?;
        let transition = waehle_transition(&liste.transitions, ziel).ok_or_else(|| {
            let name = TicketStatus::OPTIONS
                .iter()
                .find(|(id, _)| *id == ziel.id())
                .map_or("", |(_, l)| l);
            Fehler::NichtMoeglich(format!(
                "Jira bietet für {key} keine Transition nach „{name}“ an"
            ))
        })?;
        self.schicke(&transition_anfrage(key, &transition.id))
            .await
            .map(|_| ())
    }
}
