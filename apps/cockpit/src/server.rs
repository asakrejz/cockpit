//! Server mode from the browser's point of view: requests to the local backend
//! (`cockpit-server`) and the event stream. The shapes of the data are in
//! `cockpit_core::api`. No key is here and none is stored.
//!
//! The calls are grouped by area in the child modules and re-exported here, so callers write
//! `server::fetch_…`.

use cockpit_core::api::ErrorBody;
use feature_ui::t;

mod agent;
pub use agent::*;

mod cleanup;
pub use cleanup::*;

mod events;
pub use events::*;

mod github;
pub use github::*;

mod health;
pub use health::*;

mod jira;
pub use jira::*;

mod panel;
pub use panel::*;

mod setup;
pub use setup::*;

mod workspace;
pub use workspace::*;

/// Why a request to the server gave nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// The server saw the request and rejected it – with a message for the interface.
    Rejected(String),
    /// No answer: the server is not running or is not at this address.
    Unreachable,
}

impl CallError {
    pub fn message(&self) -> String {
        match self {
            CallError::Rejected(m) => m.clone(),
            CallError::Unreachable => cockpit_core::notice::server_unreachable_title()
                .render(feature_ui::lang_untracked()),
        }
    }
}

/// The message from an error response, otherwise a general one.
async fn rejection(resp: gloo_net::http::Response) -> CallError {
    let status = resp.status();
    let server_message = resp
        .json::<ErrorBody>()
        .await
        .map(|e| e.in_lang(feature_ui::lang_untracked()))
        .ok();
    CallError::Rejected(
        cockpit_core::notice::http_failure_msg(status, server_message.as_deref())
            .render(feature_ui::lang_untracked()),
    )
}

/// Like [`rejection`], but the server's own sentence wins (the GitHub calls of the assistant
/// already say in plain text what is wrong, e.g. which token problem).
async fn rejection_own(resp: gloo_net::http::Response) -> CallError {
    match resp.json::<ErrorBody>().await {
        Ok(e) if !e.error.code.is_empty() => {
            CallError::Rejected(e.in_lang(feature_ui::lang_untracked()))
        }
        _ => CallError::Rejected(t!(ServerGithubUnavailable)),
    }
}
