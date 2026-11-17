//! The setup assistant: order of the steps, reading and checking inputs, and the
//! summary in plain text. Pure logic without UI and network; the „Einrichten“ page
//! merely wires it up.

use crate::api::{
    AgentStatus, ContainerStatus, EndpointCheck, EndpointStatus, JiraCause, Provider, Setup,
};
use crate::connections::{AuthKind, ServiceStatus};

mod inputs;
pub use inputs::*;

mod log;
pub use log::*;

mod endpoint;
pub use endpoint::*;

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

    /// ID for the address (`#/setup/jira`).
    pub fn id(self) -> &'static str {
        match self {
            Step::Container => "container",
            Step::Jira => "jira",
            Step::GitHub => "github",
            Step::Model => "model",
            Step::Live => "live",
            Step::Done => "done",
        }
    }

    /// The step of an address part. The older German ids (`modell`, `fertig`) still work.
    pub fn from_id(id: &str) -> Option<Step> {
        let id = id.to_ascii_lowercase();
        match id.as_str() {
            "modell" => Some(Step::Model),
            "fertig" => Some(Step::Done),
            _ => Self::ALL.into_iter().find(|s| s.id() == id),
        }
    }

    /// The address of the step: `#/setup/jira`.
    pub fn hash(self) -> String {
        format!("#/setup/{}", self.id())
    }
}

/// Where the assistant opens without a step in the address: the first step whose summary
/// line is not good yet, otherwise the last one (the summary).
pub fn first_open_step(lines: &[Line]) -> Step {
    lines
        .iter()
        .find(|l| l.tone != Tone::Good)
        .map_or(Step::Done, |l| l.step)
}

/// The step that a message names as `Schritt „GitHub“` (or `Schritt „Modell“` …). The interface
/// uses it to put a link to that step under the message.
pub fn step_mentioned(text: &str) -> Option<Step> {
    const MARK: &str = "Schritt „";
    let start = text.find(MARK)? + MARK.len();
    let end = start + text[start..].find('“')?;
    let name = &text[start..end];
    Step::ALL
        .into_iter()
        .find(|s| s.name() == name || (*s == Step::Live && name == "Live"))
}

/// Which sign-in way the Jira step shows first: the way that is already connected; if none is,
/// OAuth only when its client ID is already entered (the user started it), otherwise the API token.
pub fn jira_default_method(jira: &ServiceStatus, oauth_client_id_set: bool) -> AuthKind {
    match (jira.connected, jira.kind) {
        (true, AuthKind::OAuth) => AuthKind::OAuth,
        (true, _) => AuthKind::Token,
        (false, _) if oauth_client_id_set => AuthKind::OAuth,
        (false, _) => AuthKind::Token,
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

/// Whether `name` is a key that the interface may delete from the .env file again: the tokens
/// („Trennen“) and the client secrets of the OAuth apps („Client-ID entfernen“). The model key
/// does not belong to it.
pub fn key_deletable(name: &str) -> bool {
    matches!(name, "jira" | "github" | "atlassian-oauth" | "github-oauth")
}

#[cfg(test)]
mod tests;
