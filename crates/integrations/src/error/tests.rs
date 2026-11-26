use super::*;

#[test]
fn key_is_removed_from_messages() {
    let f = Error::Rejected("Bad credentials ghp_geheim".into()).without_key("ghp_geheim");
    assert!(!f.to_string().contains("ghp_geheim"));
    assert!(f.to_string().contains("***"));
}

#[test]
fn problem_per_error() {
    assert_eq!(Error::NotSignedIn.problem(0), Problem::Unauthorized);
    assert_eq!(Error::NoPermission.problem(0), Problem::NoPermission);
    assert_eq!(Error::Network("x".into()).problem(0), Problem::Offline);
    assert_eq!(
        Error::RateLimit { wait_s: Some(30) }.problem(1000),
        Problem::RateLimited {
            until_ms: Some(31_000)
        }
    );
    assert_eq!(
        Error::RateLimit { wait_s: None }.problem(1000),
        Problem::RateLimited { until_ms: None }
    );
    assert!(matches!(Error::Status(500).problem(0), Problem::Other(_)));
}

/// Which kind every error is. `Error::cause` has no catch-all arm, so a new variant does not compile
/// until it is sorted; this table says where each existing one goes.
#[test]
fn cause_of_every_error() {
    use cockpit_core::{Right, TokenKind};
    let table = [
        (Error::Network("down".into()), Cause::Network),
        (Error::NotSignedIn, Cause::Token),
        (Error::NoPermission, Cause::Permission),
        (Error::RateLimit { wait_s: Some(2) }, Cause::RateLimit),
        (Error::RateLimit { wait_s: None }, Cause::RateLimit),
        (Error::NotFound, Cause::NotFound),
        (Error::TooLarge, Cause::TooLarge),
        (Error::Rejected(String::new()), Cause::Rejected),
        (Error::Rejected("no".into()), Cause::Rejected),
        (Error::Status(401), Cause::Other),
        (Error::Status(500), Cause::Other),
        (Error::Response("x".into()), Cause::BadResponse),
        (Error::Config("x".into()), Cause::BadConfig),
        (Error::NotPossible("x".into()), Cause::Other),
        (Error::Coded(msg!(IntegrationsNotFound)), Cause::Other),
        (
            Error::MissingRight {
                right: Right::IssuesWrite,
                kind: TokenKind::App,
            },
            Cause::MissingRight,
        ),
    ];
    for (e, cause) in table {
        assert_eq!(e.cause(), cause, "{e:?}");
    }
}

#[test]
fn a_rejection_gives_the_reason_of_the_service_if_there_is_one() {
    assert_eq!(
        Error::Rejected("No commits".into()).rejection(),
        Some("No commits")
    );
    assert_eq!(Error::Rejected(String::new()).rejection(), None);
    assert_eq!(Error::NotFound.rejection(), None);
    assert_eq!(Error::Response("x".into()).rejection(), None);
}

/// The state per source for every kind of error.
#[test]
fn problem_of_every_error() {
    use cockpit_core::{Right, TokenKind};
    let other = |e: Error| Problem::Other(e.msg());
    let table = [
        (Error::Network("down".into()), Problem::Offline),
        (Error::NotSignedIn, Problem::Unauthorized),
        (Error::NoPermission, Problem::NoPermission),
        (
            Error::RateLimit { wait_s: Some(2) },
            Problem::RateLimited {
                until_ms: Some(3_000),
            },
        ),
        (
            Error::RateLimit { wait_s: None },
            Problem::RateLimited { until_ms: None },
        ),
        (Error::NotFound, other(Error::NotFound)),
        (Error::TooLarge, other(Error::TooLarge)),
        (
            Error::Rejected(String::new()),
            other(Error::Rejected(String::new())),
        ),
        (
            Error::Rejected("no".into()),
            other(Error::Rejected("no".into())),
        ),
        (Error::Status(500), other(Error::Status(500))),
        (
            Error::Response("x".into()),
            other(Error::Response("x".into())),
        ),
        (Error::Config("x".into()), other(Error::Config("x".into()))),
        (
            Error::NotPossible("x".into()),
            other(Error::NotPossible("x".into())),
        ),
        (
            Error::Coded(msg!(IntegrationsNotFound)),
            other(Error::Coded(msg!(IntegrationsNotFound))),
        ),
        (
            Error::MissingRight {
                right: Right::IssuesWrite,
                kind: TokenKind::App,
            },
            Problem::MissingRight {
                right: Right::IssuesWrite,
                kind: TokenKind::App,
            },
        ),
    ];
    for (e, problem) in table {
        assert_eq!(e.problem(1_000), problem, "{e:?}");
    }
}

/// The text for logs is English, whatever language the reader has.
#[test]
fn display_is_english() {
    assert!(Error::Status(503).to_string().contains("server error"));
    assert!(Error::Status(418).to_string().contains("Unexpected status"));
    assert_eq!(
        Error::NotFound.to_string(),
        cockpit_i18n::plain(Lang::En, cockpit_i18n::Key::IntegrationsNotFound)
    );
}

/// The state that is stored for a failed fetch is a message, so it reads right in either language.
#[test]
fn other_problem_is_a_catalog_message_in_both_languages() {
    let Problem::Other(m) = Error::Status(503).problem(0) else {
        panic!("expected Problem::Other");
    };
    assert_eq!(m, Error::Status(503).msg());
    assert_eq!(m.code, "integrations.server_error");
    assert!(m.render(Lang::En).contains("server error"));
    assert!(m.render(Lang::De).contains("Serverfehler"));
}
