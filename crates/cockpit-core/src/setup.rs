//! The setup assistant: order of the steps, reading and checking inputs, and the summary in plain
//! text. Pure logic without UI and network.

use cockpit_i18n::{Lang, Msg, msg};

use crate::api::{
    AgentStatus, ContainerStatus, EndpointCheck, EndpointStatus, GitHubCause, JiraCause, Provider,
    Setup,
};
use crate::connections::{AuthKind, ServiceStatus};

mod inputs;
pub use inputs::*;

mod log;
pub use log::*;

mod endpoint;
pub use endpoint::*;

mod repo_list;
pub use repo_list::*;

mod summary;
pub use summary::*;

/// The names under which keys are stored in the .env file.
pub const KEY_NAMES: &[&str] = &[
    "github-oauth",
    "atlassian-oauth",
    "agent-key",
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
    /// Where the tickets come from: Jira, GitHub Projects (or both).
    Source,
    Jira,
    GitHub,
    /// GitHub Projects as the source of tickets.
    Projects,
    Model,
    Live,
    Done,
}

impl Step {
    pub const ALL: [Step; 8] = [
        Step::Container,
        Step::Source,
        Step::Jira,
        Step::GitHub,
        Step::Projects,
        Step::Model,
        Step::Live,
        Step::Done,
    ];

    /// Short name for the progress display.
    pub fn msg(self) -> Msg {
        match self {
            Step::Container => msg!(SetupStepContainer),
            Step::Source => msg!(SetupStepSource),
            Step::Jira => msg!(SetupStepJira),
            Step::GitHub => msg!(SetupStepGithub),
            Step::Projects => msg!(SetupStepProjects),
            Step::Model => msg!(SetupStepModel),
            Step::Live => msg!(SetupStepLive),
            Step::Done => msg!(SetupStepDone),
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

    /// Is the step part of the assistant for this choice of ticket source? The step „Jira“ only
    /// for Jira (or both), the step „GitHub Projects“ only for GitHub Projects (or both); GitHub
    /// itself (the connection for pull requests) is a step of its own either way.
    pub fn shown(self, from: crate::TicketsFrom) -> bool {
        match self {
            Step::Jira => from.uses_jira(),
            Step::Projects => from.uses_projects(),
            _ => true,
        }
    }

    /// The next step that is shown for this choice.
    pub fn next_for(self, from: crate::TicketsFrom) -> Option<Step> {
        let mut s = self.next();
        while let Some(step) = s {
            if step.shown(from) {
                return Some(step);
            }
            s = step.next();
        }
        None
    }

    /// The previous step that is shown for this choice.
    pub fn previous_for(self, from: crate::TicketsFrom) -> Option<Step> {
        let mut s = self.previous();
        while let Some(step) = s {
            if step.shown(from) {
                return Some(step);
            }
            s = step.previous();
        }
        None
    }

    /// ID for the address (`#/setup/jira`).
    pub fn id(self) -> &'static str {
        match self {
            Step::Container => "container",
            Step::Source => "source",
            Step::Jira => "jira",
            Step::GitHub => "github",
            Step::Projects => "projects",
            Step::Model => "model",
            Step::Live => "live",
            Step::Done => "done",
        }
    }

    /// The step of an address part.
    pub fn from_id(id: &str) -> Option<Step> {
        let id = id.to_ascii_lowercase();
        Self::ALL.into_iter().find(|s| s.id() == id)
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

/// The step that a message names, quoted as `Schritt „GitHub“` or `Step "GitHub"` (in either
/// language, with the older short name `Live`).
pub fn step_mentioned(text: &str) -> Option<Step> {
    const QUOTES: [(&str, char); 3] = [("Schritt „", '“'), ("step “", '”'), ("step \"", '"')];
    let lower = text.to_lowercase();
    for (mark, close) in QUOTES {
        let Some(at) = lower.find(&mark.to_lowercase()) else {
            continue;
        };
        // The lowercase text can differ in byte length for odd characters; stay on the original.
        let start = at + mark.len();
        let Some(rest) = text.get(start..) else {
            continue;
        };
        let Some(end) = rest.find(close) else {
            continue;
        };
        let name = rest[..end].trim();
        let found = Step::ALL.into_iter().find(|s| {
            Lang::ALL
                .into_iter()
                .any(|l| s.msg().render(l).eq_ignore_ascii_case(name))
                || (*s == Step::Live && name.eq_ignore_ascii_case("live"))
                || (*s == Step::Model && name.eq_ignore_ascii_case("modell"))
        });
        if found.is_some() {
            return found;
        }
    }
    None
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

/// GitHub events the webhook should send.
pub const GITHUB_WEBHOOK_EVENTS: &[&str] = &[
    "Pull requests",
    "Pull request reviews",
    "Pull request review comments",
    "Check runs",
    "Check suites",
    "Issue comments",
];

/// Jira events the webhook should send.
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
            .unwrap_or("<public address>")
    )
}

/// "Last received 3 minutes ago (pull_request)": only time and event type, never content.
pub fn webhook_received_text(last_ms: Option<u64>, event: Option<&str>, now_ms: u64) -> Msg {
    match last_ms {
        None => msg!(CoreSetupWebhookNone),
        Some(t) => {
            let when = crate::notice::ago_msg(t, now_ms);
            match event.filter(|e| !e.is_empty()) {
                Some(e) => msg!(CoreSetupWebhookEvent, when = when, event = e),
                None => msg!(CoreSetupWebhookAt, when = when),
            }
        }
    }
}

/// Whether `name` is an allowed key name.
pub fn key_name_allowed(name: &str) -> bool {
    KEY_NAMES.contains(&name)
}

/// Whether `name` is a key that may be deleted from the .env file again: the tokens and the client
/// secrets of the OAuth apps. The model key does not belong to it.
pub fn key_deletable(name: &str) -> bool {
    matches!(name, "jira" | "github" | "atlassian-oauth" | "github-oauth")
}

#[cfg(test)]
mod tests;
