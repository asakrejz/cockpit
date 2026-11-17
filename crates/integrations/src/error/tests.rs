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
    assert!(Error::Status(503).to_string().contains("Serverfehler"));
    assert!(Error::Status(418).to_string().contains("Unerwarteter"));
}
