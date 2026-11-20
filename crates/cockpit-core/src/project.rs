//! GitHub Projects (v2) as a ticket source: what a project item is, how its Status field maps
//! onto the ticket status, how keys like `acme/app#123` are written and found in texts.
//!
//! Everything here is pure – no network, no clock. The client in `cockpit-integrations` reads
//! the GraphQL answers and fills these types; the workspace only ever sees the result.

use serde::{Deserialize, Serialize};

use crate::model::PrKey;

mod keys;
pub use keys::*;

mod status;
pub use status::*;

/// A project chosen in the setup: the owner's login (user or organization) and the number
/// shown in the project's address (`https://github.com/orgs/acme/projects/3`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectRef {
    pub owner: String,
    pub number: u32,
}

impl ProjectRef {
    pub fn new(owner: &str, number: u32) -> Self {
        Self {
            owner: owner.into(),
            number,
        }
    }

    /// Reads `acme/3` (also `acme#3`). Owner names are GitHub logins: letters, digits, `-`.
    ///
    /// ```
    /// use cockpit_core::ProjectRef;
    /// assert_eq!(ProjectRef::parse("acme/3"), Some(ProjectRef::new("acme", 3)));
    /// assert_eq!(ProjectRef::parse("acme"), None);
    /// assert_eq!(ProjectRef::parse("acme/x"), None);
    /// ```
    pub fn parse(text: &str) -> Option<ProjectRef> {
        let text = text.trim();
        let (owner, number) = text.split_once('/').or_else(|| text.split_once('#'))?;
        let valid = !owner.is_empty()
            && owner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let number: u32 = number.trim().parse().ok().filter(|n| *n > 0)?;
        valid.then(|| ProjectRef::new(owner, number))
    }
}

impl std::fmt::Display for ProjectRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.number)
    }
}

/// Where the tickets come from: Jira, GitHub Projects or both.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TicketsFrom {
    #[default]
    Jira,
    GitHubProjects,
    Both,
}

impl TicketsFrom {
    pub const ALL: [TicketsFrom; 3] = [
        TicketsFrom::Jira,
        TicketsFrom::GitHubProjects,
        TicketsFrom::Both,
    ];

    pub fn id(self) -> &'static str {
        match self {
            TicketsFrom::Jira => "jira",
            TicketsFrom::GitHubProjects => "github_projects",
            TicketsFrom::Both => "both",
        }
    }

    pub fn from_id(id: &str) -> Option<TicketsFrom> {
        Self::ALL.into_iter().find(|t| t.id() == id.trim())
    }

    /// Does Jira deliver tickets?
    pub fn uses_jira(self) -> bool {
        self != TicketsFrom::GitHubProjects
    }

    /// Do GitHub Projects deliver tickets?
    pub fn uses_projects(self) -> bool {
        self != TicketsFrom::Jira
    }
}

/// Who owns a project and how the credential reaches it (for the list in the setup).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectOrigin {
    /// A project of the signed-in user.
    User,
    /// A project of an organization the user belongs to.
    Org,
    /// A project linked to one of the repos in use.
    Repo,
}

/// A project as the setup lists it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ProjectSummary {
    /// The GraphQL node ID (`PVT_…`).
    #[serde(default)]
    pub id: String,
    pub owner: String,
    pub number: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub closed: bool,
    pub origin: ProjectOrigin,
    /// Repos in use that this project is linked to (`owner/name`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<String>,
}

impl ProjectSummary {
    pub fn reference(&self) -> ProjectRef {
        ProjectRef::new(&self.owner, self.number)
    }
}

/// Kind of a project field, as far as Cockpit shows or writes it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    SingleSelect,
    Iteration,
    Number,
    Date,
    Text,
    /// Everything else (assignees, labels, milestone … – those have their own places).
    Other,
}

/// One choice of a single-select field.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SelectOption {
    pub id: String,
    pub name: String,
}

/// The definition of a project field.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FieldDef {
    pub id: String,
    pub name: String,
    pub kind: FieldKind,
    /// Single select: the choices; iteration: the iterations (ID and title).
    #[serde(default)]
    pub options: Vec<SelectOption>,
}

/// What the sync knows about a project: its fields, in particular the status field.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ProjectMeta {
    /// GraphQL node ID.
    pub id: String,
    pub owner: String,
    pub number: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub fields: Vec<FieldDef>,
    /// The field that counts as the ticket status (name from the setup, default `Status`).
    #[serde(default)]
    pub status_field: Option<String>,
}

impl ProjectMeta {
    pub fn reference(&self) -> ProjectRef {
        ProjectRef::new(&self.owner, self.number)
    }

    pub fn field(&self, id: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.id == id)
    }

    pub fn field_named(&self, name: &str) -> Option<&FieldDef> {
        self.fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name.trim()))
    }

    /// The field that holds the status: the chosen one, otherwise `Status`, as a single select.
    pub fn status_def(&self) -> Option<&FieldDef> {
        let name = self.status_field.as_deref().unwrap_or(DEFAULT_STATUS_FIELD);
        self.field_named(name)
            .filter(|f| f.kind == FieldKind::SingleSelect)
    }
}

/// The name of the field that counts as status unless the setup names another one.
pub const DEFAULT_STATUS_FIELD: &str = "Status";
/// The name of the field that holds the team unless the setup names another one.
pub const DEFAULT_TEAM_FIELD: &str = "Team";

/// A value of a project field on one item, ready to show.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FieldValue {
    pub field_id: String,
    pub name: String,
    pub kind: FieldKind,
    /// What to show: the option name, the iteration title, the number, the day, the text.
    pub text: String,
    /// Single select: the option ID. Iteration: the iteration ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
}

/// A sub-issue or the parent of an issue.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct IssueLink {
    /// `acme/app#12`
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub done: bool,
}

/// What an item knows about its place in a GitHub project. Only set for tickets of the source
/// `GitHubProject`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectItem {
    /// GraphQL node ID of the project (`PVT_…`).
    #[serde(default)]
    pub project_id: String,
    /// `acme/3`
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub project_title: String,
    /// GraphQL node ID of the item in the project (`PVTI_…`).
    #[serde(default)]
    pub item_id: String,
    /// GraphQL node ID of the issue (`I_…`); empty for a draft.
    #[serde(default)]
    pub content_id: String,
    /// A draft issue: lives only in the project, has no number.
    #[serde(default)]
    pub draft: bool,
    /// The issue is closed.
    #[serde(default)]
    pub closed: bool,
    /// The issue number in its repo; `None` for a draft.
    #[serde(default)]
    pub number: Option<u32>,
    /// Logins of the assignees.
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    /// Option ID of the status the item has now (single select), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_option: Option<String>,
    /// Values of the other project fields (without the status and the team).
    #[serde(default)]
    pub fields: Vec<FieldValue>,
    /// Pull requests that are connected to the issue (timeline, closing references).
    #[serde(default)]
    pub linked_prs: Vec<PrKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<IssueLink>,
    #[serde(default)]
    pub sub_issues: Vec<IssueLink>,
}

impl ProjectItem {
    /// The repo of the issue (`owner/name`), if the item is an issue.
    pub fn is_issue(&self) -> bool {
        !self.draft && self.number.is_some()
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_workspace;
