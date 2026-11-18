use super::*;
use axum::http::HeaderValue;

const PORT: u16 = 8787;
const LOCAL: Option<IpAddr> = Some(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

fn head(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut k = HeaderMap::new();
    for (name, value) in pairs {
        k.insert(*name, HeaderValue::from_static(value));
    }
    k
}

fn post(pairs: &[(&'static str, &'static str)]) -> Result<(), (StatusCode, cockpit_i18n::Msg)> {
    check(LOCAL, &Method::POST, &head(pairs), &[PORT], false)
}

#[test]
fn get_with_allowed_host() {
    for host in ["127.0.0.1:8787", "localhost:8787", "LOCALHOST:8787"] {
        assert!(
            check(
                LOCAL,
                &Method::GET,
                &head(&[("host", host)]),
                &[PORT],
                false
            )
            .is_ok()
        );
    }
}

#[test]
fn foreign_host_is_rejected() {
    for host in [
        "evil.example",
        "evil.example:8787",
        "127.0.0.1",
        "localhost:9999",
        "127.0.0.1.evil.example:8787",
    ] {
        let r = check(
            LOCAL,
            &Method::GET,
            &head(&[("host", host)]),
            &[PORT],
            false,
        );
        assert_eq!(r.unwrap_err().0, StatusCode::FORBIDDEN, "{host}");
    }
    assert!(check(LOCAL, &Method::GET, &HeaderMap::new(), &[PORT], false).is_err());
}

#[test]
fn only_loopback() {
    let k = head(&[("host", "127.0.0.1:8787")]);
    for ip in ["192.168.1.5", "10.0.0.1", "8.8.8.8"] {
        let peer = Some(ip.parse().unwrap());
        assert_eq!(
            check(peer, &Method::GET, &k, &[PORT], false).unwrap_err().0,
            StatusCode::FORBIDDEN
        );
    }
    assert!(
        check(
            Some("::1".parse().unwrap()),
            &Method::GET,
            &k,
            &[PORT],
            false
        )
        .is_ok()
    );
    assert!(check(None, &Method::GET, &k, &[PORT], false).is_err());
}

#[test]
fn post_needs_json() {
    let host = ("host", "127.0.0.1:8787");
    assert!(post(&[host, ("content-type", "application/json")]).is_ok());
    assert!(post(&[host, ("content-type", "Application/JSON; charset=utf-8")]).is_ok());
    for kind in [
        "text/plain",
        "application/x-www-form-urlencoded",
        "multipart/form-data",
    ] {
        assert_eq!(
            post(&[host, ("content-type", kind)]).unwrap_err().0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{kind}"
        );
    }
    assert_eq!(
        post(&[host]).unwrap_err().0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}

#[test]
fn post_origin_same_or_missing() {
    let base = [
        ("host", "127.0.0.1:8787"),
        ("content-type", "application/json"),
    ];
    let with = |origin: &'static str| {
        let mut v = base.to_vec();
        v.push(("origin", origin));
        post(&v)
    };
    assert!(post(&base).is_ok(), "without Origin");
    assert!(with("http://127.0.0.1:8787").is_ok());
    assert!(with("http://localhost:8787").is_ok());
    for fremd in [
        "https://evil.example",
        "http://127.0.0.1:9999",
        "null",
        "http://localhost",
    ] {
        assert_eq!(with(fremd).unwrap_err().0, StatusCode::FORBIDDEN, "{fremd}");
    }
}

#[test]
fn dev_port_is_only_on_request_allowed() {
    let post_from_trunk = [
        ("host", "localhost:8081"),
        ("origin", "http://localhost:8081"),
        ("content-type", "application/json"),
    ];
    assert_eq!(post(&post_from_trunk).unwrap_err().0, StatusCode::FORBIDDEN);
    let k = head(&post_from_trunk);
    assert!(check(LOCAL, &Method::POST, &k, &[PORT, 8081], true).is_ok());
    // Other ports stay out, also with the dev port.
    let k = head(&[
        ("host", "localhost:9999"),
        ("content-type", "application/json"),
    ]);
    assert!(check(LOCAL, &Method::POST, &k, &[PORT, 8081], true).is_err());
}

#[test]
fn trunk_sends_the_host_without_port() {
    let k = head(&[
        ("host", "127.0.0.1"),
        ("origin", "http://localhost:8081"),
        ("content-type", "application/json"),
    ]);
    assert!(check(LOCAL, &Method::POST, &k, &[PORT, 8081], true).is_ok());
    assert!(check(LOCAL, &Method::POST, &k, &[PORT], false).is_err());
    // A foreign name stays out also in development mode (DNS rebinding).
    let k = head(&[("host", "boese.example")]);
    assert!(check(LOCAL, &Method::GET, &k, &[PORT, 8081], true).is_err());
}
