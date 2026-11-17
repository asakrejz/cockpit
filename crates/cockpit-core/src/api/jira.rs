//! Wire shapes of the Jira endpoints.

use super::*;

/// A Jira project for the selection in the inbox.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct JiraProject {
    /// Key, e.g. `BAU`.
    pub key: String,
    /// Name, e.g. „Baukasten“.
    #[serde(default)]
    pub name: String,
}

/// Response to `GET /api/jira/projects`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraProjectsResponse {
    /// `false` if Jira is not set up or the key is missing.
    pub active: bool,
    #[serde(default)]
    pub projects: Vec<JiraProject>,
}

/// What the connection test to Jira failed on (or `Ok`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JiraCause {
    /// Jira responds and knows the token.
    Ok,
    /// Address, email or token are still missing.
    #[default]
    Details,
    /// No Jira responds at the address (404 or another page).
    Address,
    /// Jira rejects email or token (401).
    Token,
    /// The token is valid but does not have the necessary rights (403).
    Permission,
    /// No connection (network, VPN, address not found).
    Reachable,
    /// Something else (quota, Jira outage).
    Other,
}

/// Response to `POST /api/jira/check`: read only (`GET /rest/api/3/myself`), never the token.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraCheck {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub cause: JiraCause,
    /// Plain text for the UI.
    #[serde(default)]
    pub message: String,
    /// On success: the display name under which Jira knows you.
    #[serde(default)]
    pub name: Option<String>,
}

/// Jira details for `config.toml`. `None` leaves the value unchanged, an empty text removes it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraInput {
    #[serde(default)]
    pub site: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub projects: Option<Vec<String>>,
}
