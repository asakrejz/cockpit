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
