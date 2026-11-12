//! Gemeinsamer Abruf: Status in [`Fehler`] übersetzen, JSON lesen.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use reqwest::{RequestBuilder, StatusCode};

use crate::Error;

pub(crate) fn client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .user_agent(concat!("cockpit-server/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| Error::Config(e.without_url().to_string()))
}

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn header<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

/// Wie lange sollen wir pausieren? `Retry-After` (Sekunden) gewinnt; sonst der Zeitpunkt,
/// an dem das Kontingent zurückkommt (`X-RateLimit-Reset`, Sekunden seit 1970).
pub(crate) fn wait(h: &HeaderMap, now_s: u64) -> Option<u64> {
    if let Some(s) = header(h, "retry-after").and_then(|v| v.trim().parse::<u64>().ok()) {
        return Some(s);
    }
    let reset = header(h, "x-ratelimit-reset")?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(reset.saturating_sub(now_s))
}

/// Die Meldung des Dienstes aus einem Fehler-Body: GitHub (`message`), Jira
/// (`errorMessages`, `errors`). Gekürzt, damit nichts Riesiges in der Oberfläche landet.
pub(crate) fn service_message(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let mut parts: Vec<String> = Vec::new();
    // OAuth-Endpunkte (`error_description`, sonst `error`).
    if let Some(m) = json["error_description"]
        .as_str()
        .or_else(|| json["error"].as_str())
    {
        parts.push(m.to_string());
    }
    if let Some(m) = json["message"].as_str() {
        parts.push(m.to_string());
    }
    for m in json["errorMessages"].as_array().into_iter().flatten() {
        if let Some(m) = m.as_str() {
            parts.push(m.to_string());
        }
    }
    if let Some(error) = json["errors"].as_object() {
        for v in error.values() {
            if let Some(m) = v.as_str() {
                parts.push(m.to_string());
            }
        }
    }
    // GitHub: Einzelheiten stehen bei 422 in `errors` als Liste von Objekten/Texten.
    for e in json["errors"].as_array().into_iter().flatten() {
        match e {
            serde_json::Value::String(s) => parts.push(s.clone()),
            other => {
                if let Some(m) = other["message"].as_str() {
                    parts.push(m.to_string());
                }
            }
        }
    }
    let text = parts.join("; ");
    let text: String = text.chars().take(240).collect();
    (!text.trim().is_empty()).then_some(text)
}

/// Übersetzt eine Fehlerantwort. Rein, damit sich jeder Fall testen lässt.
pub(crate) fn error_from(status: StatusCode, h: &HeaderMap, body: &str, now_s: u64) -> Error {
    let limit_empty = header(h, "x-ratelimit-remaining") == Some("0");
    let pause = header(h, "retry-after").is_some();
    match status {
        StatusCode::UNAUTHORIZED => Error::NotSignedIn,
        StatusCode::TOO_MANY_REQUESTS => Error::RateLimit {
            wait_s: wait(h, now_s),
        },
        StatusCode::FORBIDDEN if limit_empty || pause => Error::RateLimit {
            wait_s: wait(h, now_s),
        },
        StatusCode::FORBIDDEN => Error::NoPermission,
        StatusCode::NOT_FOUND => Error::NotFound,
        StatusCode::BAD_REQUEST
        | StatusCode::METHOD_NOT_ALLOWED
        | StatusCode::CONFLICT
        | StatusCode::UNPROCESSABLE_ENTITY => {
            Error::Rejected(service_message(body).unwrap_or_default())
        }
        s => Error::Status(s.as_u16()),
    }
}

/// Schickt die Anfrage und liefert den Body der erfolgreichen Antwort (auch leer, z. B. 204).
pub(crate) async fn send(req: RequestBuilder) -> Result<String, Error> {
    let resp = req
        .send()
        .await
        .map_err(|e| Error::Network(e.without_url().to_string()))?;
    let status = resp.status();
    let headers = resp.headers().clone();
    let text = resp
        .text()
        .await
        .map_err(|e| Error::Network(e.without_url().to_string()))?;
    if !status.is_success() {
        return Err(error_from(status, &headers, &text, now_s()));
    }
    Ok(text)
}

/// Entfernt Schrägstriche am Ende einer Basisadresse und prüft das Schema.
pub(crate) fn base_url(url: &str) -> Result<String, Error> {
    let url = url.trim().trim_end_matches('/');
    if url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost")
    {
        Ok(url.to_string())
    } else {
        Err(Error::Config(format!(
            "Adresse muss mit https:// beginnen: {url}"
        )))
    }
}

#[cfg(test)]
mod tests {
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
        // Reset in der Vergangenheit: nicht warten.
        assert_eq!(
            wait(&header(&[("x-ratelimit-reset", "900")]), 1000),
            Some(0)
        );
        // Retry-After geht vor.
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
        let f = |s: u16, h: &HeaderMap, b: &str| {
            error_from(StatusCode::from_u16(s).unwrap(), h, b, 1000)
        };
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
        // Sekundäres Limit von GitHub: 403 mit Retry-After.
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
            f(405, &empty, r#"{"message":"Pull Request is not mergeable"}"#),
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
}
