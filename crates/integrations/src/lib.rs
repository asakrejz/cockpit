//! Clients for GitHub and Jira Cloud, the model endpoint and OAuth, and the mapping of their
//! responses onto the types from `cockpit-core`.
//!
//! Writing happens only through the actions from [`outbound`].

mod error;
mod http;
pub mod oauth;
pub mod outbound;
mod secret;
pub mod sync;
pub mod text;
pub mod time;

pub mod endpoint;
pub mod github;
pub mod jira;

pub use error::{Cause, Error};
pub use secret::Secret;
pub use sync::{GitHubService, JiraService, ProjectsService, Services};
