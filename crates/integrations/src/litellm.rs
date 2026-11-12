//! Verbindungstest zum LiteLLM-Proxy: ein einziger Aufruf, `GET <url>/v1/models` mit dem Key.
//!
//! Gemeldet wird nur, ob der Proxy antwortet, den Key annimmt und das Modell anbietet –
//! **nie der Key**. Der Aufruf folgt keinen Weiterleitungen, damit der Key nicht woanders landet.

use std::time::Duration;

use cockpit_core::api::LitellmPruefung;
use reqwest::{StatusCode, redirect::Policy};

use crate::Secret;

/// So viele angebotene Modelle nennt der Hinweis, wenn das gewünschte fehlt.
const MAX_NAMEN: usize = 8;

/// Prüft den Proxy unter `url` (ohne Schrägstrich am Ende). Ohne Key wird nichts aufgerufen.
pub async fn pruefen(url: &str, key: Option<&Secret>, modell: Option<&str>) -> LitellmPruefung {
    let mut ergebnis = LitellmPruefung {
        konfiguriert: true,
        ..Default::default()
    };
    let Some(key) = key.filter(|k| !k.is_empty()) else {
        ergebnis.hinweis = Some(
            "Es liegt kein Key im Schlüsselbund. Lege ihn im Terminal ab: cockpit-server token setzen litellm".into(),
        );
        return ergebnis;
    };
    let client = match reqwest::Client::builder()
        .user_agent(concat!("cockpit-server/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(10))
        .redirect(Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            ergebnis.hinweis = Some(format!("HTTP-Client nicht startbar: {}", e.without_url()));
            return ergebnis;
        }
    };
    let antwort = client
        .get(format!("{}/v1/models", url.trim_end_matches('/')))
        .bearer_auth(key.expose())
        .send()
        .await;
    let antwort = match antwort {
        Ok(a) => a,
        Err(e) => {
            ergebnis.hinweis = Some(if e.is_timeout() {
                format!("Der Proxy unter {url} antwortet nicht rechtzeitig.")
            } else {
                format!(
                    "Der Proxy unter {url} ist nicht erreichbar. Läuft er, und stimmt die Adresse in config.toml?"
                )
            });
            return ergebnis;
        }
    };
    ergebnis.erreichbar = true;
    let status = antwort.status();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        ergebnis.hinweis = Some(
            "Der Proxy lehnt den Key ab. Lege den richtigen Key ab: cockpit-server token setzen litellm".into(),
        );
        return ergebnis;
    }
    if !status.is_success() {
        ergebnis.hinweis = Some(format!(
            "Der Proxy antwortet mit Status {}.",
            status.as_u16()
        ));
        return ergebnis;
    }
    let Ok(json) = antwort.json::<serde_json::Value>().await else {
        ergebnis.hinweis = Some(
            "Die Antwort ist kein Modellverzeichnis. Ist das die Adresse eines LiteLLM-Proxys?"
                .into(),
        );
        return ergebnis;
    };
    ergebnis.key_ok = true;
    let namen: Vec<&str> = json["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str())
        .collect();
    match modell {
        None => {
            ergebnis.hinweis = Some(
                "Kein Modell eingestellt. Trage modell in [litellm] der config.toml ein.".into(),
            );
        }
        Some(m) if namen.contains(&m) => ergebnis.modell_vorhanden = true,
        Some(m) => {
            let mehr = if namen.len() > MAX_NAMEN { ", …" } else { "" };
            let liste: Vec<&str> = namen.iter().take(MAX_NAMEN).copied().collect();
            ergebnis.hinweis = Some(if namen.is_empty() {
                format!("Der Proxy bietet kein Modell an, „{m}“ fehlt.")
            } else {
                format!(
                    "Der Proxy bietet „{m}“ nicht an, sondern: {}{mehr}",
                    liste.join(", ")
                )
            });
        }
    }
    ergebnis
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "sk-geheim-1234567890";

    fn modelle(ids: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "object": "list",
            "data": ids.iter().map(|id| serde_json::json!({"id": id, "object": "model"})).collect::<Vec<_>>(),
        })
    }

    async fn proxy(antwort: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", format!("Bearer {KEY}").as_str()))
            .respond_with(antwort)
            .expect(1)
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn alles_in_ordnung() {
        let server = proxy(
            ResponseTemplate::new(200)
                .set_body_json(modelle(&["claude-sonnet-5-5", "claude-haiku-5-5"])),
        )
        .await;
        let p = pruefen(
            &server.uri(),
            Some(&Secret::new(KEY)),
            Some("claude-sonnet-5-5"),
        )
        .await;
        assert!(p.konfiguriert && p.erreichbar && p.key_ok && p.modell_vorhanden);
        assert_eq!(p.hinweis, None);
    }

    #[tokio::test]
    async fn modell_fehlt_und_der_hinweis_nennt_die_angebotenen() {
        let server =
            proxy(ResponseTemplate::new(200).set_body_json(modelle(&["gpt-x", "claude-a"]))).await;
        let p = pruefen(
            &format!("{}/", server.uri()),
            Some(&Secret::new(KEY)),
            Some("claude-sonnet-5-5"),
        )
        .await;
        assert!(p.erreichbar && p.key_ok && !p.modell_vorhanden);
        let hinweis = p.hinweis.unwrap();
        assert!(
            hinweis.contains("gpt-x") && hinweis.contains("claude-a"),
            "{hinweis}"
        );
    }

    #[tokio::test]
    async fn ohne_modellnamen_gibt_es_einen_hinweis() {
        let server = proxy(ResponseTemplate::new(200).set_body_json(modelle(&["a"]))).await;
        let p = pruefen(&server.uri(), Some(&Secret::new(KEY)), None).await;
        assert!(p.key_ok && !p.modell_vorhanden);
        assert!(p.hinweis.unwrap().contains("modell"));
    }

    #[tokio::test]
    async fn falscher_key_ist_erreichbar_aber_nicht_ok() {
        let server = proxy(ResponseTemplate::new(401)).await;
        let p = pruefen(&server.uri(), Some(&Secret::new(KEY)), Some("m")).await;
        assert!(p.erreichbar && !p.key_ok && !p.modell_vorhanden);
        assert!(p.hinweis.unwrap().contains("Key"));
    }

    #[tokio::test]
    async fn andere_fehler_und_unbrauchbare_antworten() {
        let server = proxy(ResponseTemplate::new(500)).await;
        let p = pruefen(&server.uri(), Some(&Secret::new(KEY)), Some("m")).await;
        assert!(p.erreichbar && !p.key_ok);
        assert!(p.hinweis.unwrap().contains("500"));

        let server = proxy(ResponseTemplate::new(200).set_body_string("<html>")).await;
        let p = pruefen(&server.uri(), Some(&Secret::new(KEY)), Some("m")).await;
        assert!(p.erreichbar && !p.key_ok);
        assert!(p.hinweis.unwrap().contains("LiteLLM"));
    }

    #[tokio::test]
    async fn weiterleitungen_werden_nicht_verfolgt() {
        let ziel = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(modelle(&["m"])))
            .expect(0)
            .mount(&ziel)
            .await;
        let server = proxy(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/v1/models", ziel.uri())),
        )
        .await;
        let p = pruefen(&server.uri(), Some(&Secret::new(KEY)), Some("m")).await;
        assert!(p.erreichbar && !p.key_ok);
    }

    #[tokio::test]
    async fn nicht_erreichbar() {
        // Ein Port, auf dem niemand lauscht.
        let frei = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let p = pruefen(
            &format!("http://127.0.0.1:{frei}"),
            Some(&Secret::new(KEY)),
            Some("m"),
        )
        .await;
        assert!(p.konfiguriert && !p.erreichbar && !p.key_ok);
        assert!(p.hinweis.unwrap().contains("nicht erreichbar"));
    }

    #[tokio::test]
    async fn ohne_key_wird_nichts_aufgerufen_und_der_key_taucht_nie_auf() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let p = pruefen(&server.uri(), None, Some("m")).await;
        assert!(p.konfiguriert && !p.erreichbar);
        assert!(
            p.hinweis
                .as_deref()
                .unwrap()
                .contains("token setzen litellm")
        );

        let server = proxy(ResponseTemplate::new(401)).await;
        let p = pruefen(&server.uri(), Some(&Secret::new(KEY)), Some("m")).await;
        let text = serde_json::to_string(&p).unwrap();
        assert!(!text.contains(KEY) && !text.contains("geheim"), "{text}");
    }
}
