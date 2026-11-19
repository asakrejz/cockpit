//! The JSON shapes between app and backend (server mode).
//!
//! * `GET /api/workspace`: the state in the shape of [`persist`](crate::persist), plus `revision`
//! * `POST /api/actions`: body [`ActionRequest`], response [`ActionResponse`] or [`ErrorBody`]
//! * `GET /api/events`: server-sent events, each event carries a [`ServerEvent`]
//! * `GET /api/oauth/{github|atlassian}/start` and `…/callback`: sign-in via OAuth (navigation of the
//!   browser, redirects to `/`); `POST …/revoke`: [`RevokeResponse`]
//! * `GET /api/jira/projects`: the Jira projects for the selection, [`JiraProjectsResponse`]
//! * `GET /api/jira/assignable?project=&query=`: people a ticket of the project can be assigned to (read only),
//!   [`JiraAssignableResponse`]
//! * `GET /api/agent/models`: the model IDs of the endpoint for the drop-down, [`ModelList`]
//! * `GET /api/agent/check`: connection test to the endpoint, [`EndpointCheck`]
//! * `POST /api/jira/check`: connection test for Jira via API token (read only), [`JiraCheck`]
//! * `DELETE /api/secrets/{name}`: deletes a token (`jira`, `github`) from the .env file
//! * `GET /api/setup`: what the setup assistant prefills, [`Setup`]
//! * `PUT /api/config`: body [`ConfigChange`], writes `config.toml`, response [`ConfigResponse`]
//! * `PUT /api/secrets/{name}`: body [`KeyInput`], stores the key in the .env file,
//!   response [`KeyResponse`] (only „set“, never the value)
//! * `GET /api/agent/image` / `POST /api/agent/image`: state or start of „build image“, [`ImageBuild`]
//! * `GET /api/agent/check/container`: the same check from a container, [`ContainerCheck`]
//! * `GET /api/agent/pull-request?run=<key>`: title and text of the pull request that would be created for the
//!   branch of a finished real run, and the one that already exists for it, [`PullRequestPreview`]
//! * `GET /api/review/commits?repo=&number=` and `GET /api/review/commit?repo=&number=&sha=[&path=]`: commits of a
//!   pull request and the diff of one commit, [`PanelCommits`], [`PanelCommitDiff`]
//! * `GET /api/health`: under `agent` an [`AgentStatus`], under `endpoint` an [`EndpointStatus`], under `container` a [`ContainerStatus`]
//!
//! Both sides use these types so that the shapes do not drift apart.

use serde::{Deserialize, Serialize};

use crate::{Action, AgentRepo, AgentRepos, LogEntry, PullRequestRef};

mod cleanup;
pub use cleanup::*;

mod jira;
pub use jira::*;

mod status;
pub use status::*;

mod webhooks;
pub use webhooks::*;

mod repos;
pub use repos::*;

mod config;
pub use config::*;

mod panel;
pub use panel::*;

/// Body of `POST /api/actions`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActionRequest {
    pub action: Action,
}

/// Response to an applied action.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActionResponse {
    /// Revision of the state after the action.
    pub revision: u64,
    /// The sentence from the log (see `Applied::summary`).
    pub summary: String,
    /// Number of new log entries.
    pub logged: usize,
}

/// Response to `POST /api/sync` („sync now“).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SyncResponse {
    /// `false` if no source is set up (Jira or GitHub with key).
    pub active: bool,
    /// Revision of the state after the sync.
    pub revision: u64,
    /// Number of new log entries.
    pub logged: usize,
}

/// Response to `POST /api/oauth/{service}/revoke`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RevokeResponse {
    /// The tokens in the .env file are deleted.
    pub disconnected: bool,
    /// The service itself revoked the token (not all offer that).
    #[serde(default)]
    pub at_service: bool,
    /// What the user can do otherwise or why the service did not revoke.
    #[serde(default)]
    pub hint: Option<String>,
    /// The same as code and parameters, rendered in the chosen language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<cockpit_i18n::Msg>,
}

/// Error response (4xx/5xx) for the UI. `error` is the German sentence (for readers that only know
/// plain text); `msg` is the same as code and parameters, which the UI shows in its language.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ErrorBody {
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<cockpit_i18n::Msg>,
}

impl ErrorBody {
    /// A sentence without a code (text from outside, for example a message of GitHub).
    pub fn text(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            msg: None,
        }
    }

    /// A message by code; the German sentence is filled in next to it.
    pub fn coded(msg: cockpit_i18n::Msg) -> Self {
        Self {
            error: msg.de(),
            msg: Some(msg),
        }
    }

    /// The text for people in `lang`.
    pub fn in_lang(&self, lang: cockpit_i18n::Lang) -> String {
        self.msg
            .as_ref()
            .map_or_else(|| self.error.clone(), |m| m.render(lang))
    }
}

/// An event on `GET /api/events`.
///
/// After every applied action one arrives with the new log entries. Right
/// after connecting one arrives with an empty list and the current revision. Whoever sees a
/// gap in the revisions (or was connected and lost the connection) reloads
/// the state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ServerEvent {
    pub revision: u64,
    #[serde(default)]
    pub entries: Vec<LogEntry>,
}

#[cfg(test)]
mod tests;
