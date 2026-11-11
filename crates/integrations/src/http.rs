//! Gemeinsamer Abruf: Status in [`Fehler`] übersetzen, JSON lesen.

use reqwest::{RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;

use crate::Fehler;

pub(crate) fn client() -> Result<reqwest::Client, Fehler> {
    reqwest::Client::builder()
        .user_agent(concat!("cockpit-server/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| Fehler::Einstellung(e.without_url().to_string()))
}

/// Schickt die Anfrage und liest die Antwort als JSON.
pub(crate) async fn lies_json<T: DeserializeOwned>(req: RequestBuilder) -> Result<T, Fehler> {
    let resp = req
        .send()
        .await
        .map_err(|e| Fehler::Netzwerk(e.without_url().to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let limit_leer = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            == Some("0");
        return Err(match status {
            StatusCode::UNAUTHORIZED => Fehler::NichtAngemeldet,
            StatusCode::TOO_MANY_REQUESTS => Fehler::RateLimit,
            StatusCode::FORBIDDEN if limit_leer => Fehler::RateLimit,
            StatusCode::FORBIDDEN => Fehler::KeineBerechtigung,
            StatusCode::NOT_FOUND => Fehler::NichtGefunden,
            s => Fehler::Status(s.as_u16()),
        });
    }
    let text = resp
        .text()
        .await
        .map_err(|e| Fehler::Netzwerk(e.without_url().to_string()))?;
    serde_json::from_str(&text).map_err(|e| Fehler::Antwort(e.to_string()))
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

    #[test]
    fn basis_url_kuerzt_und_prueft() {
        assert_eq!(
            basis_url("https://api.github.com/ ").unwrap(),
            "https://api.github.com"
        );
        assert!(basis_url("http://example.com").is_err());
        assert!(basis_url("http://127.0.0.1:1234").is_ok());
    }
}
