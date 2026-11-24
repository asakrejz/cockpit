//! What each kind of failure of a service means for the assistant: the cause named for Jira and
//! GitHub, the status and the text of the answer, and what a repo that cannot be read looks like.

use cockpit_core::api::{GitHubCause, JiraCause, RepoVisibility};
use cockpit_core::{Right, TokenKind};

use super::*;

/// Every kind of failure once.
fn every_error() -> Vec<Error> {
    vec![
        Error::Network("timeout".into()),
        Error::NotSignedIn,
        Error::NoPermission,
        Error::RateLimit { wait_s: Some(30) },
        Error::NotFound,
        Error::TooLarge,
        Error::Rejected(String::new()),
        Error::Rejected("No commits".into()),
        Error::Status(500),
        Error::Status(418),
        Error::Response("not json".into()),
        Error::Config("bad address".into()),
        Error::NotPossible("no transition".into()),
        Error::Coded(msg!(IntegrationsNotFound)),
        Error::MissingRight {
            right: Right::ProjectsRead,
            kind: TokenKind::FineGrained,
        },
    ]
}

#[test]
fn a_failed_call_to_jira_is_named_by_its_cause() {
    use crate::setup::checks::jira_cause;
    let expected = |e: &Error| match e {
        Error::NotSignedIn => JiraCause::Token,
        Error::NoPermission => JiraCause::Permission,
        Error::NotFound | Error::Response(_) | Error::Config(_) => JiraCause::Address,
        Error::Network(_) => JiraCause::Reachable,
        _ => JiraCause::Other,
    };
    for e in every_error() {
        assert_eq!(jira_cause(&e), expected(&e), "{e:?}");
    }
}

#[test]
fn a_repo_that_cannot_be_read_is_told_apart_by_the_failure() {
    use crate::setup::access::visibility_of;
    let expected = |e: &Error| match e {
        Error::NotFound => RepoVisibility::NotVisible,
        Error::NotSignedIn | Error::NoPermission => RepoVisibility::NoPermission,
        _ => RepoVisibility::Unreachable,
    };
    for e in every_error() {
        assert_eq!(visibility_of(&e), expected(&e), "{e:?}");
    }
}

#[test]
fn a_failed_call_to_github_is_answered_with_status_and_text() {
    use crate::setup::github::{cause, github_failure};
    let cause_msg = |c| github_cause_msg(c, None);
    for e in every_error() {
        let (status, text) = github_failure(&e);
        let (want_status, want_text) = match &e {
            Error::NotSignedIn => (StatusCode::UNAUTHORIZED, cause_msg(GitHubCause::Token)),
            Error::NoPermission => (StatusCode::FORBIDDEN, cause_msg(GitHubCause::Permission)),
            // The exact right that is missing.
            Error::MissingRight { .. } => (StatusCode::FORBIDDEN, e.msg()),
            Error::NotFound => (StatusCode::NOT_FOUND, msg!(ServerSetupGithubUnknownAddress)),
            Error::RateLimit { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                cause_msg(GitHubCause::RateLimit),
            ),
            Error::Network(_) => (StatusCode::BAD_GATEWAY, cause_msg(GitHubCause::Network)),
            _ => (StatusCode::BAD_GATEWAY, cause_msg(GitHubCause::Other)),
        };
        assert_eq!((status, text), (want_status, want_text), "{e:?}");

        let want_cause = match &e {
            Error::NotSignedIn => GitHubCause::Token,
            Error::NoPermission => GitHubCause::Permission,
            Error::RateLimit { .. } => GitHubCause::RateLimit,
            Error::Network(_) => GitHubCause::Network,
            _ => GitHubCause::Other,
        };
        assert_eq!(cause(&e), want_cause, "{e:?}");
    }
}
