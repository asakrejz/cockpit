//! Responses of the Jira Cloud REST API v3 – only the fields the cockpit needs.

use serde::Deserialize;
use serde_json::Value;

/// `GET /rest/api/3/myself`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Myself {
    pub account_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
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
    /// The status the transition leads to.
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
    /// Token for the next page; missing on the last one.
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
    /// ADF tree or `null`.
    #[serde(default)]
    pub description: Value,
    pub status: Status,
    #[serde(default)]
    pub assignee: Option<JiraUser>,
    #[serde(default)]
    pub reporter: Option<JiraUser>,
    #[serde(default)]
    pub updated: String,
    /// Due date as a day (`2026-11-09`).
    #[serde(default)]
    pub duedate: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// All other fields, among them the sprint field of the site (its ID differs per site).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// `GET /rest/api/3/field` – one field (only what finding the sprint field needs).
#[derive(Debug, Clone, Deserialize)]
pub struct FieldInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub schema: Option<FieldSchema>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FieldSchema {
    /// `com.pyxis.greenhopper.jira:gh-sprint` for the sprint field.
    #[serde(default)]
    pub custom: Option<String>,
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
    /// `new`, `indeterminate` or `done`
    #[serde(default)]
    pub key: String,
}

/// `GET /rest/api/3/project/search` – one page of projects.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPage {
    #[serde(default)]
    pub values: Vec<Project>,
    /// If the field is missing, the page counts as the last one.
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
