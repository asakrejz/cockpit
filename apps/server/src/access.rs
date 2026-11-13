//! Wer die Routen unter `/api` aufrufen darf – Schutz vor fremden Webseiten.
//!
//! Das Backend lauscht nur auf `127.0.0.1`, aber das allein schützt nicht vor einer Webseite
//! im selben Browser. Deshalb gilt für alle `/api`-Routen:
//!
//! * Die Gegenstelle ist eine Loopback-Adresse.
//! * Der `Host`-Header ist `127.0.0.1:<port>` oder `localhost:<port>` (gegen DNS-Rebinding:
//!   dort trägt er den fremden Namen).
//!
//! Nur für die Entwicklung mit `trunk serve` gibt es einen zweiten erlaubten Port
//! (`COCKPIT_DEV_PORT`, nur per Umgebungsvariable): Trunk leitet `/api` weiter, schickt den `Host`
//! ohne Port (`127.0.0.1`) und lässt den `Origin` der Entwicklungsseite stehen. Nur dann gelten
//! beides; ohne die Variable gilt nur der eigene Port.
//!
//! Für alles, was schreibt (nicht GET/HEAD), zusätzlich:
//!
//! * `Origin` fehlt oder ist genau eine der beiden eigenen Herkünfte (gegen fremde Seiten,
//!   die per Formular oder `fetch` senden).
//! * `Content-Type` ist `application/json`. Ein Formular kann das nicht setzen, `fetch` mit
//!   diesem Typ löst bei fremder Herkunft eine Vorabfrage aus, die wir nicht erlauben.

use std::net::{IpAddr, SocketAddr};

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cockpit_core::api::ErrorBody;

/// Prüft eine Anfrage. `peer` ist die Adresse der Gegenstelle, falls bekannt – fehlt sie,
/// wird abgelehnt.
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
        // Trunk schickt den Host ohne Port weiter; das gilt nur im Entwicklungsmodus.
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

/// Wie streng die Prüfung ist.
#[derive(Clone)]
pub struct Access {
    /// Der Port, auf dem der Server lauscht, und – nur auf ausdrücklichen Wunsch
    /// (`COCKPIT_DEV_PORT`) – der von `trunk serve`.
    pub ports: Vec<u16>,
    /// Entwicklungsmodus: Host auch ohne Port.
    pub dev: bool,
}

/// Middleware für den `/api`-Teil des Routers.
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
        // Andere Ports bleiben draußen, auch mit Dev-Port.
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
        // Ein fremder Name bleibt auch im Entwicklungsmodus draußen (DNS-Rebinding).
        let k = head(&[("host", "boese.example")]);
        assert!(check(LOCAL, &Method::GET, &k, &[PORT, 8081], true).is_err());
    }
}
