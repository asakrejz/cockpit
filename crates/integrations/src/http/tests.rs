use super::*;
use reqwest::header::{HeaderName, HeaderValue};

fn header(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.insert(
            HeaderName::from_bytes(k.as_bytes()).unwrap(),
            HeaderValue::from_str(v).unwrap(),
        );
    }
    h
}

#[test]
fn base_url_is_trimmed_and_checked() {
    assert_eq!(
        base_url("https://api.github.com/ ").unwrap(),
        "https://api.github.com"
    );
    assert!(base_url("http://example.com").is_err());
    assert!(base_url("http://127.0.0.1:1234").is_ok());
}

#[test]
fn wait_from_the_headers() {
    assert_eq!(wait(&header(&[("retry-after", "42")]), 1000), Some(42));
    assert_eq!(
        wait(&header(&[("x-ratelimit-reset", "1090")]), 1000),
        Some(90)
    );
    // Reset in the past: do not wait.
    assert_eq!(
        wait(&header(&[("x-ratelimit-reset", "900")]), 1000),
        Some(0)
    );
    // Retry-After comes first.
    assert_eq!(
        wait(
            &header(&[("retry-after", "5"), ("x-ratelimit-reset", "2000")]),
            1000
        ),
        Some(5)
    );
    assert_eq!(wait(&HeaderMap::new(), 1000), None);
}

#[test]
fn status_codes_become_errors() {
    let empty = HeaderMap::new();
    let f =
        |s: u16, h: &HeaderMap, b: &str| error_from(StatusCode::from_u16(s).unwrap(), h, b, 1000);
    assert_eq!(f(401, &empty, ""), Error::NotSignedIn);
    assert_eq!(f(403, &empty, ""), Error::NoPermission);
    assert_eq!(f(404, &empty, ""), Error::NotFound);
    assert_eq!(f(500, &empty, ""), Error::Status(500));
    assert_eq!(
        f(
            403,
            &header(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1030")
            ]),
            ""
        ),
        Error::RateLimit { wait_s: Some(30) }
    );
    // GitHub secondary limit: 403 with Retry-After.
    assert_eq!(
        f(403, &header(&[("retry-after", "60")]), ""),
        Error::RateLimit { wait_s: Some(60) }
    );
    assert_eq!(
        f(429, &header(&[("retry-after", "7")]), ""),
        Error::RateLimit { wait_s: Some(7) }
    );
    assert_eq!(
        f(
            422,
            &empty,
            r#"{"message":"Validation Failed","errors":[{"message":"Zeile fehlt"}]}"#
        ),
        Error::Rejected("Validation Failed; Zeile fehlt".into())
    );
    assert_eq!(
        f(
            405,
            &empty,
            r#"{"message":"Pull Request is not mergeable"}"#
        ),
        Error::Rejected("Pull Request is not mergeable".into())
    );
    assert_eq!(
        f(
            400,
            &empty,
            r#"{"errorMessages":["Feld ungültig"],"errors":{"x":"y"}}"#
        ),
        Error::Rejected("Feld ungültig; y".into())
    );
    assert_eq!(f(409, &empty, "kein json"), Error::Rejected(String::new()));
}
