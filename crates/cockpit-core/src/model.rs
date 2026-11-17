//! Data model: inbox entries and what belongs to them.

use serde::{Deserialize, Serialize};

mod status;
pub use status::*;

mod people;
pub use people::*;

mod pull_request;
pub use pull_request::*;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Jira,
    GitHub,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Jira => "Jira",
            Source::GitHub => "GitHub",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ticket,
    PullRequest,
}

/// Does the entry belong to me or to someone on the team?
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Mine,
    Team,
}

/// Inbox filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Mine,
    Team,
    /// Snoozed entries (see [`crate::attention`]).
    Snoozed,
}

impl Filter {
    /// ID and label, in display order.
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("all", "Alle"),
        ("mine", "Ich"),
        ("team", "Team"),
        ("snoozed", "Zurückgestellt"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Mine => "mine",
            Filter::Team => "team",
            Filter::Snoozed => "snoozed",
        }
    }

    pub fn from_id(id: &str) -> Option<Filter> {
        [Filter::All, Filter::Mine, Filter::Team, Filter::Snoozed]
            .into_iter()
            .find(|f| f.id() == id)
    }

    pub fn matches(self, scope: Scope) -> bool {
        match self {
            Filter::All | Filter::Snoozed => true,
            Filter::Mine => scope == Scope::Mine,
            Filter::Team => scope == Scope::Team,
        }
    }
}

/// Inbox filter by source: all, only Jira or only GitHub.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceFilter {
    #[default]
    All,
    Jira,
    GitHub,
}

impl SourceFilter {
    /// ID and label, in display order.
    pub const OPTIONS: &'static [(&'static str, &'static str)] =
        &[("all", "Alle"), ("jira", "Jira"), ("github", "GitHub")];

    pub fn id(self) -> &'static str {
        match self {
            SourceFilter::All => "all",
            SourceFilter::Jira => "jira",
            SourceFilter::GitHub => "github",
        }
    }

    pub fn from_id(id: &str) -> Option<SourceFilter> {
        [SourceFilter::All, SourceFilter::Jira, SourceFilter::GitHub]
            .into_iter()
            .find(|f| f.id() == id)
    }

    pub fn matches(self, source: Source) -> bool {
        match self {
            SourceFilter::All => true,
            SourceFilter::Jira => source == Source::Jira,
            SourceFilter::GitHub => source == Source::GitHub,
        }
    }
}

/// An inbox entry: a Jira ticket or a GitHub pull request.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Internal ID, e.g. `bau142`.
    #[serde(default)]
    pub id: String,
    pub kind: Kind,
    pub source: Source,
    /// Visible key: `BAU-142` or `#87`.
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub reason: ReasonKind,
    #[serde(default)]
    pub reason_text: String,
    #[serde(default)]
    pub time: String,
    pub scope: Scope,
    pub owner: Person,
    /// Only relevant for tickets.
    #[serde(default)]
    pub status: TicketStatus,
    /// Only relevant for tickets.
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub pr: Option<PrRef>,
    /// Note from the agent, e.g. „Vom Agenten umgesetzt · Tests bestanden“.
    #[serde(default)]
    pub agent_note: Option<String>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// Done – needs no attention any more.
    #[serde(default)]
    pub read: bool,
    /// Due date as a day (`2026-11-09`), for tickets only.
    #[serde(default)]
    pub due: Option<String>,
    /// The repo (`owner/name`) of a pull request; empty for tickets.
    #[serde(default)]
    pub repo: Option<String>,
    /// Labels of the service (Jira labels, GitHub labels).
    #[serde(default)]
    pub labels: Vec<String>,
}

impl Item {
    pub fn is_ticket(&self) -> bool {
        self.kind == Kind::Ticket
    }

    /// The Jira project of a ticket: the part of the key before the hyphen
    /// (`BAU-142` → `BAU`). Pull requests and other entries have none.
    ///
    /// ```
    /// use cockpit_core::demo;
    /// let ws = demo::workspace();
    /// assert_eq!(ws.item_by_key("BAU-142").unwrap().project(), Some("BAU"));
    /// assert_eq!(ws.item_by_key("#87").unwrap().project(), None);
    /// ```
    pub fn project(&self) -> Option<&str> {
        if self.source != Source::Jira {
            return None;
        }
        self.key
            .split_once('-')
            .map(|(p, _)| p)
            .filter(|p| is_project_key(p))
    }

    /// The key as shown in lists: `BAU-142` for tickets; for pull requests `acme/app#87`
    /// (with the repo, because `#87` alone can exist in two repos), or `#87` if the repo is unknown.
    pub fn display_key(&self) -> String {
        match &self.pr {
            Some(p) if self.kind == Kind::PullRequest && !p.repo.is_empty() => p.key().to_string(),
            _ => self.key.clone(),
        }
    }

    /// Does a run belong to this entry? A run is named after the ticket key; a follow-up job
    /// on a pull request without ticket after the pull request including its repo
    /// (`acme/app#87`), because `#87` alone could be any repo's.
    pub fn matches_run_key(&self, run_key: &str) -> bool {
        self.key == run_key
            || (self.kind == Kind::PullRequest
                && self.pr.as_ref().is_some_and(|p| {
                    PrKey::parse(run_key).is_some_and(|k| !k.repo.is_empty() && k.same(&p.key()))
                }))
    }

    /// The status for filters: for tickets their own, for pull requests the one the pull
    /// request suggests (merged → Done, review requested → Review, otherwise In progress).
    pub fn effective_status(&self) -> TicketStatus {
        match (&self.kind, &self.pr) {
            (Kind::PullRequest, Some(pr)) => pr.target_status(),
            _ => self.status,
        }
    }
}

/// Is this a valid Jira project key? A letter first, then letters, digits
/// or `_`. This way nothing slips into a JQL query.
pub fn is_project_key(key: &str) -> bool {
    key.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests;
