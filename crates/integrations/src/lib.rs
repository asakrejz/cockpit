//! Clients for GitHub and Jira Cloud – and the mapping of their responses onto the
//! types from `cockpit-core`.
//!
//! * [`github`]: pull requests, files with patch, comments, check status
//! * [`jira`]: issues via JQL, comments (Atlassian Document Format → text)
//! * [`endpoint`]: connection test and model list of the model endpoint (proxy like LiteLLM or directly at Anthropic)
//! * [`oauth`]: sign-in via OAuth (GitHub App, Atlassian 3LO), refresh and revoke
//! * [`Secret`]: a key that never shows up in `Debug` output or logs
//!
//! * [`outbound`]: which action becomes which call to which service (pure, tested)
//! * [`sync`]: fetching all sources for the sync in the backend
//!
//! Writing happens only through the actions from [`outbound`]. The mapping functions are pure
//! functions and are tested with stored sample responses; the clients with
//! a mock server. Nothing has been checked against the real services yet.

mod error;
mod http;
pub mod oauth;
pub mod outbound;
mod secret;
pub mod sync;
pub mod text;
pub mod time;
pub mod translate;

pub mod endpoint;
pub mod github;
pub mod jira;

pub use error::Error;
pub use secret::Secret;
pub use sync::{GitHubService, JiraService, Services};
