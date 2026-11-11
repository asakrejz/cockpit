//! Gemeinsamer Abruf: Status in [`Fehler`] übersetzen, JSON lesen.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use reqwest::{RequestBuilder, StatusCode};

use crate::Fehler;

pub(crate) fn client() -> Result<reqwest::Client, Fehler> {
    reqwest::Client::builder()
        .user_agent(concat!("cockpit-server/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| Fehler::Einstellung(e.without_url().to_string()))
}

fn jetzt_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn kopfzeile<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

/// Wie lange sollen wir pausieren? `Retry-After` (Sekunden) gewinnt; sonst der Zeitpunkt,
/// an dem das Kontingent zurückkommt (`X-RateLimit-Reset`, Sekunden seit 1970).
pub(crate) fn wartezeit(h: &HeaderMap, jetzt_s: u64) -> Option<u64> {
    if let Some(s) = kopfzeile(h, "retry-after").and_then(|v| v.trim().parse::<u64>().ok()) {
        return Some(s);
    }
    let reset = kopfzeile(h, "x-ratelimit-reset")?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(reset.saturating_sub(jetzt_s))
}

/// Die Meldung des Dienstes aus einem Fehler-Body: GitHub (`message`), Jira
/// (`errorMessages`, `errors`). Gekürzt, damit nichts Riesiges in der Oberfläche landet.
pub(crate) fn dienst_meldung(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let mut teile: Vec<String> = Vec::new();
    if let Some(m) = json["message"].as_str() {
        teile.push(m.to_string());
    }
    for m in json["errorMessages"].as_array().into_iter().flatten() {
        if let Some(m) = m.as_str() {
            teile.push(m.to_string());
        }
    }
    if let Some(fehler) = json["errors"].as_object() {
        for v in fehler.values() {
            if let Some(m) = v.as_str() {
                teile.push(m.to_string());
            }
        }
    }
    // GitHub: Einzelheiten stehen bei 422 in `errors` als Liste von Objekten/Texten.
    for e in json["errors"].as_array().into_iter().flatten() {
        match e {
            serde_json::Value::String(s) => teile.push(s.clone()),
            other => {
                if let Some(m) = other["message"].as_str() {
                    teile.push(m.to_string());
                }
            }
        }
    }
    let text = teile.join("; ");
    let text: String = text.chars().take(240).collect();
    (!text.trim().is_empty()).then_some(text)
}

/// Übersetzt eine Fehlerantwort. Rein, damit sich jeder Fall testen lässt.
pub(crate) fn fehler_aus(status: StatusCode, h: &HeaderMap, body: &str, jetzt_s: u64) -> Fehler {
    let limit_leer = kopfzeile(h, "x-ratelimit-remaining") == Some("0");
    let pause = kopfzeile(h, "retry-after").is_some();
    match status {
        StatusCode::UNAUTHORIZED => Fehler::NichtAngemeldet,
        StatusCode::TOO_MANY_REQUESTS => Fehler::RateLimit {
            warten_s: wartezeit(h, jetzt_s),
        },
        StatusCode::FORBIDDEN if limit_leer || pause => Fehler::RateLimit {
            warten_s: wartezeit(h, jetzt_s),
        },
        StatusCode::FORBIDDEN => Fehler::KeineBerechtigung,
        StatusCode::NOT_FOUND => Fehler::NichtGefunden,
        StatusCode::BAD_REQUEST
        | StatusCode::METHOD_NOT_ALLOWED
        | StatusCode::CONFLICT
        | StatusCode::UNPROCESSABLE_ENTITY => {
            Fehler::Abgelehnt(dienst_meldung(body).unwrap_or_default())
        }
        s => Fehler::Status(s.as_u16()),
    }
}

/// Schickt die Anfrage und liefert den Body der erfolgreichen Antwort (auch leer, z. B. 204).
pub(crate) async fn sende(req: RequestBuilder) -> Result<String, Fehler> {
    let resp = req
        .send()
        .await
        .map_err(|e| Fehler::Netzwerk(e.without_url().to_string()))?;
    let status = resp.status();
    let headers = resp.headers().clone();
    let text = resp
        .text()
        .await
        .map_err(|e| Fehler::Netzwerk(e.without_url().to_string()))?;
    if !status.is_success() {
        return Err(fehler_aus(status, &headers, &text, jetzt_s()));
    }
    Ok(text)
}

/// Entfernt Schrägstriche am Ende einer Basisadresse und prüft das Schema.
pub(crate) fn basis_url(url: &str) -> Result<String, Fehler> {
    let url = url.trim().trim_end_matches('/');
    if url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost")
    {
        Ok(url.to_string())
    } else {
        Err(Fehler::Einstellung(format!(
            "Adresse muss mit https:// beginnen: {url}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};

    fn kopf(paare: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in paare {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn basis_url_kuerzt_und_prueft() {
        assert_eq!(
            basis_url("https://api.github.com/ ").unwrap(),
            "https://api.github.com"
        );
        assert!(basis_url("http://example.com").is_err());
        assert!(basis_url("http://127.0.0.1:1234").is_ok());
    }

    #[test]
    fn wartezeit_aus_den_kopfzeilen() {
        assert_eq!(wartezeit(&kopf(&[("retry-after", "42")]), 1000), Some(42));
        assert_eq!(
            wartezeit(&kopf(&[("x-ratelimit-reset", "1090")]), 1000),
            Some(90)
        );
        // Reset in der Vergangenheit: nicht warten.
        assert_eq!(
            wartezeit(&kopf(&[("x-ratelimit-reset", "900")]), 1000),
            Some(0)
        );
        // Retry-After geht vor.
        assert_eq!(
            wartezeit(
                &kopf(&[("retry-after", "5"), ("x-ratelimit-reset", "2000")]),
                1000
            ),
            Some(5)
        );
        assert_eq!(wartezeit(&HeaderMap::new(), 1000), None);
    }

    #[test]
    fn statuscodes_werden_zu_fehlern() {
        let leer = HeaderMap::new();
        let f = |s: u16, h: &HeaderMap, b: &str| {
            fehler_aus(StatusCode::from_u16(s).unwrap(), h, b, 1000)
        };
        assert_eq!(f(401, &leer, ""), Fehler::NichtAngemeldet);
        assert_eq!(f(403, &leer, ""), Fehler::KeineBerechtigung);
        assert_eq!(f(404, &leer, ""), Fehler::NichtGefunden);
        assert_eq!(f(500, &leer, ""), Fehler::Status(500));
        assert_eq!(
            f(
                403,
                &kopf(&[
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset", "1030")
                ]),
                ""
            ),
            Fehler::RateLimit { warten_s: Some(30) }
        );
        // Sekundäres Limit von GitHub: 403 mit Retry-After.
        assert_eq!(
            f(403, &kopf(&[("retry-after", "60")]), ""),
            Fehler::RateLimit { warten_s: Some(60) }
        );
        assert_eq!(
            f(429, &kopf(&[("retry-after", "7")]), ""),
            Fehler::RateLimit { warten_s: Some(7) }
        );
        assert_eq!(
            f(
                422,
                &leer,
                r#"{"message":"Validation Failed","errors":[{"message":"Zeile fehlt"}]}"#
            ),
            Fehler::Abgelehnt("Validation Failed; Zeile fehlt".into())
        );
        assert_eq!(
            f(405, &leer, r#"{"message":"Pull Request is not mergeable"}"#),
            Fehler::Abgelehnt("Pull Request is not mergeable".into())
        );
        assert_eq!(
            f(
                400,
                &leer,
                r#"{"errorMessages":["Feld ungültig"],"errors":{"x":"y"}}"#
            ),
            Fehler::Abgelehnt("Feld ungültig; y".into())
        );
        assert_eq!(f(409, &leer, "kein json"), Fehler::Abgelehnt(String::new()));
    }
}
