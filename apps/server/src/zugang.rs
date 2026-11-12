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
pub fn pruefen(
    peer: Option<IpAddr>,
    methode: &Method,
    kopf: &HeaderMap,
    ports: &[u16],
    dev: bool,
) -> Result<(), (StatusCode, &'static str)> {
    if !peer.is_some_and(|ip| ip.is_loopback()) {
        return Err((
            StatusCode::FORBIDDEN,
            "Nur Anfragen von diesem Rechner sind erlaubt",
        ));
    }
    let host = kopf.get(HOST).and_then(|h| h.to_str().ok());
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
    if matches!(*methode, Method::GET | Method::HEAD) {
        return Ok(());
    }
    if let Some(origin) = kopf.get(ORIGIN) {
        let origin = origin.to_str().unwrap_or("").to_ascii_lowercase();
        let eigene = ports.iter().any(|p| {
            origin == format!("http://127.0.0.1:{p}") || origin == format!("http://localhost:{p}")
        });
        if !eigene {
            return Err((StatusCode::FORBIDDEN, "Fremde Herkunft"));
        }
    }
    let ist_json = kopf
        .get(CONTENT_TYPE)
        .and_then(|c| c.to_str().ok())
        .is_some_and(|c| {
            c.split(';')
                .next()
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"))
        });
    if !ist_json {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Erwartet wird Content-Type: application/json",
        ));
    }
    Ok(())
}

/// Wie streng die Prüfung ist.
#[derive(Clone)]
pub struct Zugang {
    /// Der Port, auf dem der Server lauscht, und – nur auf ausdrücklichen Wunsch
    /// (`COCKPIT_DEV_PORT`) – der von `trunk serve`.
    pub ports: Vec<u16>,
    /// Entwicklungsmodus: Host auch ohne Port.
    pub dev: bool,
}

/// Middleware für den `/api`-Teil des Routers.
pub async fn schuetzen(State(zugang): State<Zugang>, request: Request, next: Next) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    match pruefen(
        peer,
        request.method(),
        request.headers(),
        &zugang.ports,
        zugang.dev,
    ) {
        Ok(()) => next.run(request).await,
        Err((status, meldung)) => (
            status,
            Json(ErrorBody {
                error: meldung.into(),
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
    const LOKAL: Option<IpAddr> = Some(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    fn kopf(paare: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut k = HeaderMap::new();
        for (name, wert) in paare {
            k.insert(*name, HeaderValue::from_static(wert));
        }
        k
    }

    fn post(paare: &[(&'static str, &'static str)]) -> Result<(), (StatusCode, &'static str)> {
        pruefen(LOKAL, &Method::POST, &kopf(paare), &[PORT], false)
    }

    #[test]
    fn get_mit_erlaubtem_host() {
        for host in ["127.0.0.1:8787", "localhost:8787", "LOCALHOST:8787"] {
            assert!(
                pruefen(
                    LOKAL,
                    &Method::GET,
                    &kopf(&[("host", host)]),
                    &[PORT],
                    false
                )
                .is_ok()
            );
        }
    }

    #[test]
    fn fremder_host_wird_abgelehnt() {
        for host in [
            "evil.example",
            "evil.example:8787",
            "127.0.0.1",
            "localhost:9999",
            "127.0.0.1.evil.example:8787",
        ] {
            let r = pruefen(
                LOKAL,
                &Method::GET,
                &kopf(&[("host", host)]),
                &[PORT],
                false,
            );
            assert_eq!(r.unwrap_err().0, StatusCode::FORBIDDEN, "{host}");
        }
        assert!(pruefen(LOKAL, &Method::GET, &HeaderMap::new(), &[PORT], false).is_err());
    }

    #[test]
    fn nur_loopback() {
        let k = kopf(&[("host", "127.0.0.1:8787")]);
        for ip in ["192.168.1.5", "10.0.0.1", "8.8.8.8"] {
            let peer = Some(ip.parse().unwrap());
            assert_eq!(
                pruefen(peer, &Method::GET, &k, &[PORT], false)
                    .unwrap_err()
                    .0,
                StatusCode::FORBIDDEN
            );
        }
        assert!(
            pruefen(
                Some("::1".parse().unwrap()),
                &Method::GET,
                &k,
                &[PORT],
                false
            )
            .is_ok()
        );
        assert!(pruefen(None, &Method::GET, &k, &[PORT], false).is_err());
    }

    #[test]
    fn post_braucht_json() {
        let host = ("host", "127.0.0.1:8787");
        assert!(post(&[host, ("content-type", "application/json")]).is_ok());
        assert!(post(&[host, ("content-type", "Application/JSON; charset=utf-8")]).is_ok());
        for typ in [
            "text/plain",
            "application/x-www-form-urlencoded",
            "multipart/form-data",
        ] {
            assert_eq!(
                post(&[host, ("content-type", typ)]).unwrap_err().0,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "{typ}"
            );
        }
        assert_eq!(
            post(&[host]).unwrap_err().0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }

    #[test]
    fn post_origin_gleich_oder_fehlend() {
        let basis = [
            ("host", "127.0.0.1:8787"),
            ("content-type", "application/json"),
        ];
        let mit = |origin: &'static str| {
            let mut v = basis.to_vec();
            v.push(("origin", origin));
            post(&v)
        };
        assert!(post(&basis).is_ok(), "ohne Origin");
        assert!(mit("http://127.0.0.1:8787").is_ok());
        assert!(mit("http://localhost:8787").is_ok());
        for fremd in [
            "https://evil.example",
            "http://127.0.0.1:9999",
            "null",
            "http://localhost",
        ] {
            assert_eq!(mit(fremd).unwrap_err().0, StatusCode::FORBIDDEN, "{fremd}");
        }
    }

    #[test]
    fn dev_port_ist_nur_auf_wunsch_erlaubt() {
        let post_von_trunk = [
            ("host", "localhost:8081"),
            ("origin", "http://localhost:8081"),
            ("content-type", "application/json"),
        ];
        assert_eq!(post(&post_von_trunk).unwrap_err().0, StatusCode::FORBIDDEN);
        let k = kopf(&post_von_trunk);
        assert!(pruefen(LOKAL, &Method::POST, &k, &[PORT, 8081], true).is_ok());
        // Andere Ports bleiben draußen, auch mit Dev-Port.
        let k = kopf(&[
            ("host", "localhost:9999"),
            ("content-type", "application/json"),
        ]);
        assert!(pruefen(LOKAL, &Method::POST, &k, &[PORT, 8081], true).is_err());
    }

    #[test]
    fn trunk_schickt_den_host_ohne_port() {
        let k = kopf(&[
            ("host", "127.0.0.1"),
            ("origin", "http://localhost:8081"),
            ("content-type", "application/json"),
        ]);
        assert!(pruefen(LOKAL, &Method::POST, &k, &[PORT, 8081], true).is_ok());
        assert!(pruefen(LOKAL, &Method::POST, &k, &[PORT], false).is_err());
        // Ein fremder Name bleibt auch im Entwicklungsmodus draußen (DNS-Rebinding).
        let k = kopf(&[("host", "boese.example")]);
        assert!(pruefen(LOKAL, &Method::GET, &k, &[PORT, 8081], true).is_err());
    }
}
