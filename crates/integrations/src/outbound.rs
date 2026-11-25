//! Actions with an effect on the outside: which [`cockpit_core::Action`] goes to which service, and what does
//! the HTTP call look like? Everything here is pure (no network, no clock) and is tested without a server.
//!
//! * [`plan_call`] tells for an action what has to be done at the service ([`Outbound`]) – or `None`
//!   if the action stays local (other actions, entries from the sample data, source off).
//! * [`Outbound::route`] names the service that sends the call. `Services::execute` picks its
//!   client, and the client's `execute` is the only way out: it matches every [`Outbound`] variant
//!   (no catch-all arm), so a new variant does not compile until it says how it is sent.
//! * The requests themselves are built by the `*_request` functions in the `outbound` module. A call that
//!   takes several requests (Jira transition, branch, re-run check) is a flow of the client; its pure
//!   parts (selection, bodies) live there too.
//!
//! Only an action that requires `Actor::User` (`Action::required_actor`) can become a call here.
//! `Workspace::apply` checks that and the other rules (e.g. „only you may merge“) **before** the call;
//! the test `only_actions_that_require_the_user_become_calls` walks through every action.

mod plan;
mod request;
mod review;

pub use plan::plan_call;
pub use request::{
    branch_delete_request, branch_request, check_again_request, choose_transition,
    github_comment_request, jira_comment_request, merge_request, pull_request_request,
    review_request, transition_request,
};
pub use review::{LineComment, ReviewData, review_data};

use cockpit_core::{MergeMethod, TicketStatus};
use serde_json::Value;

/// What the server has in terms of services.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Targets {
    pub github: bool,
    pub jira: bool,
    /// GitHub Projects are set up as a source of tickets.
    pub projects: bool,
    /// The repo a branch for a Jira ticket goes to: the default repo of the agent
    /// (`[agent] repo`, or the only configured one). `None` when there is none or there are
    /// several without a default; the plan then asks the user to set one.
    pub default_repo: Option<String>,
}

/// What has to be done at the service.
#[derive(Clone, Debug, PartialEq)]
pub enum Outbound {
    JiraComment {
        key: String,
        text: String,
    },
    JiraStatus {
        key: String,
        status: TicketStatus,
    },
    GitHubComment {
        repo: String,
        number: u32,
        text: String,
    },
    GitHubBranch {
        repo: String,
        name: String,
    },
    GitHubReview {
        repo: String,
        number: u32,
        review: ReviewData,
    },
    GitHubMerge {
        repo: String,
        number: u32,
        method: MergeMethod,
        /// The commit we have seen – a new push makes the merge fail.
        sha: String,
    },
    GitHubRecheck {
        repo: String,
        number: u32,
        check: String,
    },
    /// GitHub Projects: set a field of an item (the status is the field of its column).
    ProjectField {
        project_id: String,
        item_id: String,
        field_id: String,
        /// `None` clears the field.
        value: Option<cockpit_core::FieldInput>,
    },
    /// GitHub Projects: title and body of a draft issue.
    DraftEdit {
        draft_id: String,
        title: String,
        body: String,
    },
    /// GitHub Projects: a change at the issue itself (comment, state, text, assignee, label).
    Issue {
        repo: String,
        number: u32,
        write: crate::github::projects::IssueWrite,
    },
}

/// The service that sends a call (and the client of the configured sources it goes through).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Jira,
    /// Pull requests, branches and checks.
    GitHub,
    /// GitHub Projects: through the client of the project source, if there is one.
    Projects,
}

impl Outbound {
    /// Which service sends this call.
    pub fn route(&self) -> Route {
        match self {
            Outbound::JiraComment { .. } | Outbound::JiraStatus { .. } => Route::Jira,
            Outbound::GitHubComment { .. }
            | Outbound::GitHubBranch { .. }
            | Outbound::GitHubReview { .. }
            | Outbound::GitHubMerge { .. }
            | Outbound::GitHubRecheck { .. } => Route::GitHub,
            Outbound::ProjectField { .. } | Outbound::DraftEdit { .. } | Outbound::Issue { .. } => {
                Route::Projects
            }
        }
    }

    /// Name of the service for messages.
    pub fn service(&self) -> &'static str {
        match self.route() {
            Route::Jira => "Jira",
            Route::GitHub | Route::Projects => "GitHub",
        }
    }
}

/// A single HTTP call.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub method: &'static str,
    pub path: String,
    pub body: Option<Value>,
}

/// Why an action cannot be mapped to the outside. The message is what people read; `Display`
/// is its English text, for logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanError(pub cockpit_i18n::Msg);

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.render(cockpit_i18n::Lang::En))
    }
}

#[cfg(test)]
mod tests;
