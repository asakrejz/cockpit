//! The setup assistant: order of the steps, reading and checking inputs, and the
//! summary in plain text. Pure logic without UI and network; the „Einrichten“ page
//! merely wires it up.

use crate::api::{
    AgentStatus, ContainerStatus, EndpointCheck, EndpointStatus, GitHubCause, JiraCause, Provider,
    Setup,
};
use crate::connections::{AuthKind, ServiceStatus};

mod inputs;
pub use inputs::*;

mod endpoint;
pub use endpoint::*;

mod repo_list;
pub use repo_list::*;

mod summary;
pub use summary::*;

/// The names under which the backend stores keys in the .env file (`PUT /api/secrets/{name}`).
pub const KEY_NAMES: &[&str] = &[
    "github-oauth",
    "atlassian-oauth",
    "agent-key",
    "litellm",
    "jira",
    "github",
    "github-webhook",
    "jira-webhook",
];

/// Where to create an API token at Atlassian.
pub const JIRA_TOKEN_PAGE: &str = "https://id.atlassian.com/manage-profile/security/api-tokens";

/// Longest allowed key (characters). Real tokens are far shorter.
pub const MAX_KEYS: usize = 4096;

/// A step of the assistant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Container,
    Jira,
    GitHub,
    Model,
    Live,
    Done,
}

impl Step {
    pub const ALL: [Step; 6] = [
        Step::Container,
        Step::Jira,
        Step::GitHub,
        Step::Model,
        Step::Live,
        Step::Done,
    ];

    /// Short name for the progress display.
    pub fn name(self) -> &'static str {
        match self {
            Step::Container => "Container",
            Step::Jira => "Jira",
            Step::GitHub => "GitHub",
            Step::Model => "Modell",
            Step::Live => "Live-Update",
            Step::Done => "Fertig",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn next(self) -> Option<Step> {
        Self::ALL.get(self.index() + 1).copied()
    }

    pub fn previous(self) -> Option<Step> {
        self.index().checked_sub(1).map(|i| Self::ALL[i])
    }

    /// ID for the address (`#/einrichten/jira`).
    pub fn id(self) -> &'static str {
        match self {
            Step::Container => "container",
            Step::Jira => "jira",
            Step::GitHub => "github",
            Step::Model => "modell",
            Step::Live => "live",
            Step::Done => "fertig",
        }
    }

    pub fn from_id(id: &str) -> Option<Step> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// `true` if nothing at all is set up yet – then the assistant appears on first start.
pub fn nothing_set_up(
    setup: &Setup,
    jira: &ServiceStatus,
    github: &ServiceStatus,
    endpoint: &EndpointStatus,
) -> bool {
    !(jira.connected
        || github.connected
        || endpoint.configured
        || endpoint.key
        || setup.atlassian_client_id.is_some()
        || setup.github_client_id.is_some()
        || !setup.github_repos.is_empty()
        || !setup.jira_site.is_empty())
}

/// GitHub events the webhook should send (Settings › Webhooks › „Let me select individual events“).
pub const GITHUB_WEBHOOK_EVENTS: &[&str] = &[
    "Pull requests",
    "Pull request reviews",
    "Pull request review comments",
    "Check runs",
    "Check suites",
    "Issue comments",
];

/// Jira events the webhook should send (Admin › System › WebHooks).
pub const JIRA_WEBHOOK_EVENTS: &[&str] = &[
    "Issue: created, updated, deleted",
    "Comment: created, updated, deleted",
];

/// The address to enter at GitHub or Jira: the public address of the tunnel plus the route. Without
/// an address a placeholder shows how it will look.
pub fn webhook_target(public_url: Option<&str>, path: &str) -> String {
    format!(
        "{}{path}",
        public_url
            .filter(|u| !u.is_empty())
            .unwrap_or("<öffentliche Adresse>")
    )
}

/// "Zuletzt empfangen vor 3 Min. (pull_request)" – only time and event type, never content.
pub fn webhook_received_text(last_ms: Option<u64>, event: Option<&str>, now_ms: u64) -> String {
    match last_ms {
        None => "Noch nichts empfangen (seit dem Start von Cockpit).".into(),
        Some(t) => {
            let when = crate::notice::ago(t, now_ms);
            match event.filter(|e| !e.is_empty()) {
                Some(e) => format!("Zuletzt empfangen {when} ({e})."),
                None => format!("Zuletzt empfangen {when}."),
            }
        }
    }
}

/// Whether `name` is an allowed key name.
pub fn key_name_allowed(name: &str) -> bool {
    KEY_NAMES.contains(&name)
}

/// Whether `name` is a token that „Trennen“ may delete from the .env file. Client secrets and
/// the model key do not belong to it.
pub fn key_deletable(name: &str) -> bool {
    matches!(name, "jira" | "github")
}

#[cfg(test)]
mod tests;
