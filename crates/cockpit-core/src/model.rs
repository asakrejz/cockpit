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
    /// GitHub pull requests (the review inbox).
    GitHub,
    /// Issues and draft issues of GitHub Projects (v2), used as tickets. A source of its own
    /// because the sync, the removal of vanished entries and the sync state are per source: the
    /// pull request sync must never remove an issue, and the project sync never a pull request.
    GitHubProject,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Jira => "Jira",
            Source::GitHub => "GitHub",
            Source::GitHubProject => "GitHub Projects",
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
    /// The IDs, in display order; the interface words them.
    pub const IDS: &'static [&'static str] = &["all", "mine", "team", "snoozed"];

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SourceFilter {
    #[default]
    All,
    Jira,
    GitHub,
    /// Tickets from GitHub Projects.
    GitHubProject,
}

impl SourceFilter {
    /// The IDs, in display order; the interface words them.
    pub const IDS: &'static [&'static str] = &["all", "jira", "github", "project"];

    pub fn id(self) -> &'static str {
        match self {
            SourceFilter::All => "all",
            SourceFilter::Jira => "jira",
            SourceFilter::GitHub => "github",
            SourceFilter::GitHubProject => "project",
        }
    }

    pub fn from_id(id: &str) -> Option<SourceFilter> {
        [
            SourceFilter::All,
            SourceFilter::Jira,
            SourceFilter::GitHub,
            SourceFilter::GitHubProject,
        ]
        .into_iter()
        .find(|f| f.id() == id)
    }

    pub fn matches(self, source: Source) -> bool {
        match self {
            SourceFilter::All => true,
            SourceFilter::Jira => source == Source::Jira,
            SourceFilter::GitHub => source == Source::GitHub,
            SourceFilter::GitHubProject => source == Source::GitHubProject,
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
    /// The description as formatted Jira text (ADF); `None` = only [`Item::description`]
    /// exists (older data, pull requests, an edited description).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_rich: Option<crate::adf::Doc>,
    pub reason: ReasonKind,
    /// The reason line as code and parameters; the interface renders it in its language.
    /// `None` = only [`Item::reason_text`] (older data), until the next sync replaces the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_msg: Option<cockpit_i18n::Msg>,
    /// The reason line of older data as plain German text; new entries carry
    /// [`Item::reason_msg`] and leave this empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason_text: String,
    /// The time of older data as plain German text (such as "five minutes ago"); new entries carry
    /// [`Item::updated_ms`] and leave this empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
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
    /// The note of the agent as plain German text; only a state saved earlier holds it, new
    /// notes are in [`Item::agent_note_msg`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_note: Option<String>,
    /// Note from the agent as code and parameters (e.g. "Implemented by the agent · tests
    /// passed"); the UI renders it in its language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_note_msg: Option<cockpit_i18n::Msg>,
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
    /// Jira only: the sprint the ticket is in (the active one if it is in several).
    /// `None` = no sprint (backlog) or no sprint data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprint: Option<Sprint>,
    /// Jira only: the issue type (`Bug`, `Story` …), `None` = unknown. Gives `{type}` of a branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<String>,
    /// Jira only: who reported the ticket (display name; `Du` for me), `None` = unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reporter: Option<String>,
    /// Jira only: I watch the ticket.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub watching: bool,
    /// Jira only: the team of the ticket (Atlassian „Team“ field). `None` = no team (or not
    /// loaded: old data, no such field on the site).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamRef>,
    /// Last activity (milliseconds since 1970), `None` = unknown. The service's update time; an
    /// action of Cockpit that touches the entry moves it to that moment. The interface shows it
    /// as "5 minutes ago".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_ms: Option<u64>,
    /// Where the entry lives at its service (`https://acme.atlassian.net/browse/BAU-142`,
    /// the pull request page). `None` = not known, then the UI shows no link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Jira only: fields and attachments beyond the description. `None` = not loaded (older
    /// data, demo without extras).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<crate::details::Details>,
    /// GitHub Projects only: where the ticket sits in its project (IDs for writing, the values
    /// of the project fields, linked pull requests, parent and sub-issues).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<crate::project::ProjectItem>,
    /// People the ticket is assigned to besides the owner (GitHub allows several), in the name
    /// shown in lists (`Du` for me). Empty = at most the owner.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub co_assignees: Vec<String>,
    /// Milestone of an issue, `None` = none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
}

/// The team of a ticket: the Atlassian team's ID (a UUID) and its name.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TeamRef {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

impl TeamRef {
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

/// State of a Jira sprint.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SprintState {
    Active,
    Closed,
    Future,
}

impl SprintState {
    /// Reads Jira's `state` (any case); unknown values give `None`.
    pub fn parse(text: &str) -> Option<SprintState> {
        match text.trim().to_lowercase().as_str() {
            "active" => Some(SprintState::Active),
            "closed" => Some(SprintState::Closed),
            "future" => Some(SprintState::Future),
            _ => None,
        }
    }
}

/// The sprint of a ticket as the sync delivers it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Sprint {
    #[serde(default)]
    pub name: String,
    pub state: SprintState,
    /// Start and end as Jira writes them (ISO timestamp), empty = unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub end: String,
}

impl Item {
    /// Sets why the entry needs attention: the kind and the sentence for the row.
    pub fn set_reason(&mut self, reason: ReasonKind, msg: cockpit_i18n::Msg) {
        self.reason = reason;
        self.reason_msg = Some(msg);
        self.reason_text.clear();
    }

    /// The reason line of the row: the message, for older data its stored text.
    pub fn reason_line(&self) -> cockpit_i18n::Msg {
        self.reason_msg
            .clone()
            .unwrap_or_else(|| cockpit_i18n::Msg::raw(self.reason_text.clone()))
    }

    /// Sets the note of the agent.
    pub fn set_note(&mut self, msg: cockpit_i18n::Msg) {
        self.agent_note = None;
        self.agent_note_msg = Some(msg);
    }

    /// The note of the agent in `lang`: the message, for a note of older data (or a code that
    /// no longer exists) its stored text.
    pub fn agent_note_in(&self, lang: cockpit_i18n::Lang) -> Option<String> {
        let stored = self.agent_note.as_deref().unwrap_or_default();
        match &self.agent_note_msg {
            Some(m) => Some(m.render_or(lang, stored)),
            None => self.agent_note.clone(),
        }
    }

    /// Removes the note of the agent.
    pub fn clear_note(&mut self) {
        self.agent_note = None;
        self.agent_note_msg = None;
    }

    /// Where the entry lives at its service, for the link next to its number: the address the
    /// sync stored, for a pull request of a known repo otherwise its page at github.com.
    /// Only `http(s)` addresses; `None` = no link.
    pub fn web_url(&self) -> Option<String> {
        if let Some(u) = self.url.as_deref().and_then(crate::adf::safe_url) {
            return Some(u.to_string());
        }
        match (&self.kind, &self.pr) {
            (Kind::PullRequest, Some(pr)) => pr.github_url(),
            _ => None,
        }
    }

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

    /// The Jira project this entry belongs to: for a ticket its own ([`Item::project`]); for a
    /// pull request the project of the first ticket key in its branch name or title
    /// (`feature/bau-142-x` → `BAU`), `None` if there is none.
    pub fn ticket_project(&self) -> Option<String> {
        if self.kind == Kind::Ticket {
            return self.project().map(str::to_string);
        }
        let branch = self.branch.as_deref().unwrap_or_default();
        [branch, self.title.as_str()].iter().find_map(|text| {
            crate::link::ticket_keys(text)
                .first()
                .and_then(|k| k.split_once('-'))
                .map(|(p, _)| p.to_string())
                .filter(|p| is_project_key(p))
        })
    }

    /// Does the entry belong to me (assigned, own, review requested), as opposed to the team?
    pub fn is_mine(&self) -> bool {
        self.scope == Scope::Mine
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
