//! Who may call the routes under `/api` – protection against foreign web pages.
//!
//! The backend only listens on `127.0.0.1`, but that alone does not protect against a web page
//! in the same browser. Therefore the following applies to all `/api` routes:
//!
//! * The peer is a loopback address.
//! * The `Host` header is `127.0.0.1:<port>` or `localhost:<port>` (against DNS rebinding:
//!   there it carries the foreign name).
//!
//! Only for development with `trunk serve` there is a second allowed port
//! (`COCKPIT_DEV_PORT`, only by environment variable): Trunk forwards `/api`, sends the `Host`
//! without a port (`127.0.0.1`) and leaves the `Origin` of the development page as it is. Only then do
//! both apply; without the variable only the own port applies.
//!
//! For everything that writes (not GET/HEAD), additionally:
//!
//! * `Origin` is missing or is exactly one of the two own origins (against foreign pages
//!   that send by form or `fetch`).
//! * `Content-Type` is `application/json`. A form cannot set that, `fetch` with
//!   this type triggers a preflight for a foreign origin, which we do not allow.

use std::net::{IpAddr, SocketAddr};

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cockpit_core::api::ErrorBody;

/// Checks a request. `peer` is the address of the peer if known – if it is missing,
/// the request is rejected.
pub fn check(
    peer: Option<IpAddr>,
    method: &Method,
    head: &HeaderMap,
    ports: &[u16],
    dev: bool,
) -> Result<(), (StatusCode, &'static str)> {
    if !peer.is_some_and(|ip| ip.is_loopback()) {
        return Err((
            StatusCode::FORBIDDEN,
            "Nur Anfragen von diesem Rechner sind erlaubt",
        ));
    }
    let host = head.get(HOST).and_then(|h| h.to_str().ok());
    let host_ok = host.is_some_and(|h| {
        let h = h.to_ascii_lowercase();
        // Trunk forwards the host without a port; that only applies in development mode.
        (dev && (h == "127.0.0.1" || h == "localhost"))
            || ports
                .iter()
                .any(|p| h == format!("127.0.0.1:{p}") || h == format!("localhost:{p}"))
    });
    if !host_ok {
        return Err((StatusCode::FORBIDDEN, "Unerwarteter Host"));
    }
    if matches!(*method, Method::GET | Method::HEAD) {
        return Ok(());
    }
    if let Some(origin) = head.get(ORIGIN) {
        let origin = origin.to_str().unwrap_or("").to_ascii_lowercase();
        let own = ports.iter().any(|p| {
            origin == format!("http://127.0.0.1:{p}") || origin == format!("http://localhost:{p}")
        });
        if !own {
            return Err((StatusCode::FORBIDDEN, "Fremde Herkunft"));
        }
    }
    let is_json = head
        .get(CONTENT_TYPE)
        .and_then(|c| c.to_str().ok())
        .is_some_and(|c| {
            c.split(';')
                .next()
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"))
        });
    if !is_json {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Erwartet wird Content-Type: application/json",
        ));
    }
    Ok(())
}

/// How strict the check is.
#[derive(Clone)]
pub struct Access {
    /// The port on which the server listens and – only on explicit request
    /// (`COCKPIT_DEV_PORT`) – that of `trunk serve`.
    pub ports: Vec<u16>,
    /// Development mode: host also without a port.
    pub dev: bool,
}

/// Middleware for the `/api` part of the router.
pub async fn protect(State(access): State<Access>, request: Request, next: Next) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    match check(
        peer,
        request.method(),
        request.headers(),
        &access.ports,
        access.dev,
    ) {
        Ok(()) => next.run(request).await,
        Err((status, message)) => (
            status,
            Json(ErrorBody {
                error: message.into(),
            }),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    const PORT: u16 = 8787;
    const LOCAL: Option<IpAddr> = Some(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    fn head(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut k = HeaderMap::new();
        for (name, wert) in pairs {
            k.insert(*name, HeaderValue::from_static(wert));
        }
        k
    }

    fn post(pairs: &[(&'static str, &'static str)]) -> Result<(), (StatusCode, &'static str)> {
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
        assert!(post(&base).is_ok(), "ohne Origin");
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
}
