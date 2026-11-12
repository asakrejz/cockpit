//! Antworten der Jira-Cloud-REST-API v3 – nur die Felder, die das Cockpit braucht.

use serde::Deserialize;
use serde_json::Value;

/// `GET /rest/api/3/myself`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Myself {
    pub account_id: String,
}

/// `GET /rest/api/3/issue/{key}/transitions`
#[derive(Debug, Clone, Deserialize)]
pub struct TransitionList {
    #[serde(default)]
    pub transitions: Vec<Transition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Transition {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Der Status, in den die Transition führt.
    pub to: Status,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraUser {
    pub account_id: String,
    #[serde(default)]
    pub display_name: String,
}

/// `GET /rest/api/3/search/jql`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    #[serde(default)]
    pub issues: Vec<Issue>,
    /// Token für die nächste Seite; fehlt auf der letzten.
    #[serde(default)]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Issue {
    pub key: String,
    pub fields: Fields,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Fields {
    #[serde(default)]
    pub summary: String,
    /// ADF-Baum oder `null`.
    #[serde(default)]
    pub description: Value,
    pub status: Status,
    #[serde(default)]
    pub assignee: Option<JiraUser>,
    #[serde(default)]
    pub reporter: Option<JiraUser>,
    #[serde(default)]
    pub updated: String,
    /// Fälligkeitsdatum als Tag (`2026-11-09`).
    #[serde(default)]
    pub duedate: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status_category: Option<StatusCategory>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusCategory {
    /// `new`, `indeterminate` oder `done`
    #[serde(default)]
    pub key: String,
}

/// `GET /rest/api/3/project/search` – eine Seite der Projekte.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPage {
    #[serde(default)]
    pub values: Vec<Project>,
    /// Fehlt das Feld, gilt die Seite als letzte.
    #[serde(default = "ja")]
    pub is_last: bool,
}

fn ja() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub key: String,
    #[serde(default)]
    pub name: String,
}

/// `GET /rest/api/3/issue/{key}/comment`
#[derive(Debug, Clone, Deserialize)]
pub struct CommentPage {
    #[serde(default)]
    pub comments: Vec<JiraComment>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JiraComment {
    pub author: JiraUser,
    #[serde(default)]
    pub body: Value,
    #[serde(default)]
    pub created: String,
}
