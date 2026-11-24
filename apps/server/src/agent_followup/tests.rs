use cockpit_core::{Right, TokenKind};

use super::*;

/// One error of every kind that does not have its own text in [`github_message`].
fn others() -> Vec<ServiceError> {
    vec![
        ServiceError::Network("timeout".into()),
        ServiceError::RateLimit { wait_s: Some(30) },
        ServiceError::TooLarge,
        ServiceError::Status(500),
        ServiceError::Status(418),
        ServiceError::Response("not json".into()),
        ServiceError::Config("bad address".into()),
        ServiceError::NotPossible("no transition".into()),
        ServiceError::Coded(msg!(IntegrationsNotFound)),
        ServiceError::MissingRight {
            right: Right::IssuesWrite,
            kind: TokenKind::FineGrained,
        },
    ]
}

#[test]
fn a_github_failure_names_what_failed_and_what_to_do() {
    let what = || msg!(ServerAgentWhatPullRead);
    assert_eq!(
        github_message(what(), &ServiceError::NotSignedIn),
        msg!(ServerAgentGithub401, what = what())
    );
    assert_eq!(
        github_message(what(), &ServiceError::NoPermission),
        msg!(ServerAgentGithub403, what = what())
    );
    assert_eq!(
        github_message(what(), &ServiceError::NotFound),
        msg!(ServerAgentGithub404, what = what())
    );
    assert_eq!(
        github_message(what(), &ServiceError::Rejected(String::new())),
        msg!(ServerAgentGithub422, what = what())
    );
    assert_eq!(
        github_message(what(), &ServiceError::Rejected("No commits".into())),
        msg!(
            ServerAgentGithub422Detail,
            what = what(),
            detail = Msg::raw("No commits")
        )
    );
    for e in others() {
        assert_eq!(
            github_message(what(), &e),
            msg!(ServerAgentGithubOther, what = what(), error = e.msg()),
            "{e:?}"
        );
    }
}
