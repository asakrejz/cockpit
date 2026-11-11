//! Antworten der Jira-Cloud-REST-API v3 – nur die Felder, die das Cockpit braucht.

use serde::Deserialize;
use serde_json::Value;

/// `GET /rest/api/3/myself`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Myself {
    pub account_id: String,
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
pub struct SearchResult {
    #[serde(default)]
    pub issues: Vec<Issue>,
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
