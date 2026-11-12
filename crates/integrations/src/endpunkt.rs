//! Der Modell-Endpunkt des Agenten: Verbindungstest und Modellliste mit einem einzigen Aufruf,
//! `GET <url>/v1/models`.
//!
//! * Ein Proxy oder anderer kompatibler Endpunkt (LiteLLM u. a.): Header `Authorization: Bearer <Key>`.
//! * Direkt bei Anthropic (`https://api.anthropic.com/v1/models`): `x-api-key` und `anthropic-version`.
//!
//! Gemeldet wird nur, was der Endpunkt antwortet (Modell-IDs) – **nie der Key**. Der Aufruf folgt
//! keinen Weiterleitungen, damit der Key nicht woanders landet, hat ein Zeitlimit und liest nur
//! eine begrenzte Menge Antwort.

use std::time::Duration;

use cockpit_core::api::{EndpunktPruefung, MAX_MODELLE, Modellliste};
use reqwest::{StatusCode, redirect::Policy};

use crate::Secret;

/// Die Adresse der Anthropic-API (ohne Schrägstrich am Ende).
pub const ANTHROPIC_API: &str = "https://api.anthropic.com";
/// Die API-Version, die Anthropic bei jedem Aufruf verlangt.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// So viele angebotene Modelle nennt der Hinweis, wenn das gewünschte fehlt.
const MAX_NAMEN: usize = 8;
/// Mehr Antwort wird nicht gelesen (die Liste trägt nur IDs; echte Listen sind weit kleiner).
const MAX_ANTWORT: usize = 4 * 1024 * 1024;
const ZEITLIMIT: Duration = Duration::from_secs(10);

/// Wohin der Aufruf geht und wie er sich ausweist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ziel {
    /// Ohne Schrägstrich am Ende.
    pub basis: String,
    /// Direkt bei Anthropic: `x-api-key` statt `Authorization: Bearer`.
    pub direkt: bool,
}

impl Ziel {
    /// Mit Adresse ein Proxy oder kompatibler Endpunkt, ohne Adresse direkt bei Anthropic.
    /// `direkt_basis` ersetzt die Anthropic-Adresse (nur für Tests).
    pub fn aus(url: Option<&str>, direkt_basis: Option<&str>) -> Ziel {
        match url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) => Ziel {
                basis: u.trim_end_matches('/').to_string(),
                direkt: false,
            },
            None => Ziel {
                basis: direkt_basis
                    .unwrap_or(ANTHROPIC_API)
                    .trim_end_matches('/')
                    .to_string(),
                direkt: true,
            },
        }
    }

    fn wer(&self) -> String {
        if self.direkt {
            "Anthropic".into()
        } else {
            format!("Der Endpunkt unter {}", self.basis)
        }
    }

    fn key_rat(&self) -> &'static str {
        "Speichere den richtigen Key im Assistenten (Schritt „Modell“) oder im Terminal: cockpit-server token setzen agent-key"
    }
}

/// Warum der Abruf keine Liste gebracht hat.
enum Misslungen {
    KeinKey,
    ClientNichtStartbar(String),
    Zeitueberschreitung,
    NichtErreichbar,
    KeyAbgelehnt,
    Status(u16),
    KeinVerzeichnis,
}

impl Misslungen {
    fn erreichbar(&self) -> bool {
        matches!(
            self,
            Misslungen::KeyAbgelehnt | Misslungen::Status(_) | Misslungen::KeinVerzeichnis
        )
    }

    fn text(&self, ziel: &Ziel) -> String {
        let wer = ziel.wer();
        match self {
            Misslungen::KeinKey => {
                format!("Es liegt kein Key im Schlüsselbund. {}.", ziel.key_rat())
            }
            Misslungen::ClientNichtStartbar(e) => format!("HTTP-Client nicht startbar: {e}"),
            Misslungen::Zeitueberschreitung => format!("{wer} antwortet nicht rechtzeitig."),
            Misslungen::NichtErreichbar => {
                if ziel.direkt {
                    "Anthropic ist nicht erreichbar. Hast du eine Internetverbindung, und lässt dein Netz api.anthropic.com durch?".into()
                } else {
                    format!("{wer} ist nicht erreichbar. Läuft er, und stimmt die Adresse?")
                }
            }
            Misslungen::KeyAbgelehnt => format!("{wer} lehnt den Key ab. {}.", ziel.key_rat()),
            Misslungen::Status(s) => format!("{wer} antwortet mit Status {s}."),
            Misslungen::KeinVerzeichnis => {
                if ziel.direkt {
                    "Die Antwort von Anthropic ist kein Modellverzeichnis.".into()
                } else {
                    format!(
                        "Die Antwort von {} ist kein Modellverzeichnis. Ist das die Adresse eines Anthropic-kompatiblen Endpunkts (z. B. eines LiteLLM-Proxys)?",
                        ziel.basis
                    )
                }
            }
        }
    }
}

/// Eine brauchbare Modell-ID: nicht leer, nicht zu lang, ohne Leerraum und Steuerzeichen.
fn id_brauchbar(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= 200
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Sortiert die IDs (Claude-Modelle zuerst, dann alphabetisch), entfernt Doppelte und kappt auf [`MAX_MODELLE`].
pub fn ids_ordnen(mut ids: Vec<String>) -> Vec<String> {
    ids.sort_by(|a, b| {
        let claude = |s: &str| !s.to_lowercase().contains("claude");
        (claude(a), a).cmp(&(claude(b), b))
    });
    ids.dedup();
    ids.truncate(MAX_MODELLE);
    ids
}

async fn abrufen(ziel: &Ziel, key: Option<&Secret>) -> Result<Vec<String>, Misslungen> {
    let key = key.filter(|k| !k.is_empty()).ok_or(Misslungen::KeinKey)?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("cockpit-server/", env!("CARGO_PKG_VERSION")))
        .timeout(ZEITLIMIT)
        .redirect(Policy::none())
        .build()
        .map_err(|e| Misslungen::ClientNichtStartbar(e.without_url().to_string()))?;
    let mut anfrage = client.get(format!("{}/v1/models", ziel.basis));
    anfrage = if ziel.direkt {
        // Die Liste ist seitenweise; 1000 ist das Höchste, das Anthropic erlaubt.
        anfrage
            .query(&[("limit", "1000")])
            .header("x-api-key", key.expose())
            .header("anthropic-version", ANTHROPIC_VERSION)
    } else {
        anfrage.bearer_auth(key.expose())
    };
    let mut antwort = anfrage.send().await.map_err(|e| {
        if e.is_timeout() {
            Misslungen::Zeitueberschreitung
        } else {
            Misslungen::NichtErreichbar
        }
    })?;
    let status = antwort.status();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(Misslungen::KeyAbgelehnt);
    }
    if !status.is_success() {
        return Err(Misslungen::Status(status.as_u16()));
    }
    let mut body: Vec<u8> = Vec::new();
    loop {
        match antwort.chunk().await {
            Ok(Some(teil)) => {
                if body.len() + teil.len() > MAX_ANTWORT {
                    return Err(Misslungen::KeinVerzeichnis);
                }
                body.extend_from_slice(&teil);
            }
            Ok(None) => break,
            Err(e) if e.is_timeout() => return Err(Misslungen::Zeitueberschreitung),
            Err(_) => return Err(Misslungen::KeinVerzeichnis),
        }
    }
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| Misslungen::KeinVerzeichnis)?;
    let daten = json["data"].as_array().ok_or(Misslungen::KeinVerzeichnis)?;
    Ok(ids_ordnen(
        daten
            .iter()
            .filter_map(|m| m["id"].as_str())
            .filter(|id| id_brauchbar(id))
            .map(str::to_string)
            .collect(),
    ))
}

/// Die Modell-IDs des Endpunkts für das Drop-down. Ohne Key wird nichts aufgerufen.
pub async fn modelle_laden(ziel: &Ziel, key: Option<&Secret>) -> Modellliste {
    match abrufen(ziel, key).await {
        Ok(modelle) if modelle.is_empty() => Modellliste {
            erreichbar: true,
            key_ok: true,
            hinweis: Some(format!(
                "{} bietet keine Modelle an. Trage den Modellnamen von Hand ein.",
                ziel.wer()
            )),
            ..Default::default()
        },
        Ok(modelle) => Modellliste {
            geladen: true,
            erreichbar: true,
            key_ok: true,
            modelle,
            hinweis: None,
        },
        Err(m) => Modellliste {
            erreichbar: m.erreichbar(),
            hinweis: Some(m.text(ziel)),
            ..Default::default()
        },
    }
}

/// Prüft den Endpunkt: erreichbar, Key angenommen, Modell vorhanden. `modell` ist der eingestellte Name.
pub async fn pruefen(ziel: &Ziel, key: Option<&Secret>, modell: Option<&str>) -> EndpunktPruefung {
    let mut ergebnis = EndpunktPruefung {
        konfiguriert: true,
        ..Default::default()
    };
    let namen = match abrufen(ziel, key).await {
        Ok(n) => n,
        Err(m) => {
            ergebnis.erreichbar = m.erreichbar();
            ergebnis.hinweis = Some(m.text(ziel));
            return ergebnis;
        }
    };
    ergebnis.erreichbar = true;
    ergebnis.key_ok = true;
    match modell {
        None => {
            ergebnis.hinweis =
                Some("Kein Modell eingestellt. Wähle eines im Schritt „Modell“.".into());
        }
        Some(m) if namen.iter().any(|n| n == m) => ergebnis.modell_vorhanden = true,
        Some(m) => {
            let mehr = if namen.len() > MAX_NAMEN { ", …" } else { "" };
            let liste: Vec<&str> = namen.iter().take(MAX_NAMEN).map(String::as_str).collect();
            ergebnis.hinweis = Some(if namen.is_empty() {
                format!("{} bietet kein Modell an, „{m}“ fehlt.", ziel.wer())
            } else {
                format!(
                    "{} bietet „{m}“ nicht an, sondern: {}{mehr}",
                    ziel.wer(),
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
        proxy_n(antwort, 1).await
    }

    async fn proxy_n(antwort: ResponseTemplate, n: u64) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", format!("Bearer {KEY}").as_str()))
            .respond_with(antwort)
            .expect(n)
            .mount(&server)
            .await;
        server
    }

    fn ziel(server: &MockServer) -> Ziel {
        Ziel::aus(Some(&server.uri()), None)
    }

    #[test]
    fn target_depends_on_the_url() {
        let z = Ziel::aus(Some(" http://localhost:4000/ "), None);
        assert_eq!(
            (z.basis.as_str(), z.direkt),
            ("http://localhost:4000", false)
        );
        let z = Ziel::aus(None, None);
        assert_eq!((z.basis.as_str(), z.direkt), (ANTHROPIC_API, true));
        assert!(Ziel::aus(Some("  "), Some("http://127.0.0.1:9/")).direkt);
        assert_eq!(
            Ziel::aus(Some(""), Some("http://127.0.0.1:9/")).basis,
            "http://127.0.0.1:9"
        );
    }

    #[tokio::test]
    async fn list_is_sorted_with_claude_first() {
        let server = proxy(ResponseTemplate::new(200).set_body_json(modelle(&[
            "gpt-x",
            "claude-sonnet-5-5",
            "azure/b",
            "Claude-Haiku",
            "claude-sonnet-5-5",
            "claude-a",
        ])))
        .await;
        let l = modelle_laden(&ziel(&server), Some(&Secret::new(KEY))).await;
        assert!(l.geladen && l.erreichbar && l.key_ok);
        assert_eq!(
            l.modelle,
            [
                "Claude-Haiku",
                "claude-a",
                "claude-sonnet-5-5",
                "azure/b",
                "gpt-x"
            ]
        );
        assert_eq!(l.hinweis, None);
    }

    #[tokio::test]
    async fn only_ids_come_back_and_odd_entries_are_dropped() {
        let body = serde_json::json!({"data": [
            {"id": "ok-1", "owned_by": "geheim-intern", "pricing": {"x": 1}},
            {"id": ""}, {"id": 7}, {"nichts": true}, {"id": "mit leerzeichen"}, {"id": "x\ny"},
            "kein-objekt",
        ]});
        let server = proxy(ResponseTemplate::new(200).set_body_json(body)).await;
        let l = modelle_laden(&ziel(&server), Some(&Secret::new(KEY))).await;
        assert_eq!(l.modelle, ["ok-1"]);
        let text = serde_json::to_string(&l).unwrap();
        assert!(!text.contains("geheim"), "{text}");
    }

    #[tokio::test]
    async fn a_huge_list_is_capped() {
        let ids: Vec<String> = (0..1500).map(|i| format!("modell-{i:04}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let server = proxy(ResponseTemplate::new(200).set_body_json(modelle(&refs))).await;
        let l = modelle_laden(&ziel(&server), Some(&Secret::new(KEY))).await;
        assert_eq!(l.modelle.len(), MAX_MODELLE);
        assert_eq!(l.modelle[0], "modell-0000");
    }

    #[tokio::test]
    async fn a_response_beyond_the_size_limit_is_refused() {
        let gross = "x".repeat(MAX_ANTWORT + 10);
        let server = proxy(ResponseTemplate::new(200).set_body_string(gross)).await;
        let l = modelle_laden(&ziel(&server), Some(&Secret::new(KEY))).await;
        assert!(!l.geladen && l.modelle.is_empty());
        assert!(l.hinweis.unwrap().contains("kein Modellverzeichnis"));
    }

    #[tokio::test]
    async fn rejected_key_empty_list_and_broken_json_say_so() {
        let server = proxy(ResponseTemplate::new(401)).await;
        let l = modelle_laden(&ziel(&server), Some(&Secret::new(KEY))).await;
        assert!(l.erreichbar && !l.key_ok && !l.geladen);
        assert!(l.hinweis.unwrap().contains("lehnt den Key ab"));

        let server = proxy(ResponseTemplate::new(200).set_body_json(modelle(&[]))).await;
        let l = modelle_laden(&ziel(&server), Some(&Secret::new(KEY))).await;
        assert!(l.erreichbar && l.key_ok && !l.geladen && l.modelle.is_empty());
        assert!(l.hinweis.unwrap().contains("keine Modelle"));

        for kaputt in ["<html>", "{\"data\": 5}", "{\"modelle\": []}", ""] {
            let server = proxy(ResponseTemplate::new(200).set_body_string(kaputt)).await;
            let l = modelle_laden(&ziel(&server), Some(&Secret::new(KEY))).await;
            assert!(!l.geladen && l.erreichbar, "{kaputt}");
            assert!(
                l.hinweis.unwrap().contains("kein Modellverzeichnis"),
                "{kaputt}"
            );
        }

        let server = proxy(ResponseTemplate::new(500)).await;
        let l = modelle_laden(&ziel(&server), Some(&Secret::new(KEY))).await;
        assert!(l.hinweis.unwrap().contains("500"));
    }

    #[tokio::test]
    async fn direct_calls_use_x_api_key_and_the_version_header() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("x-api-key", KEY))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(modelle(&["claude-opus-5-5"])))
            .expect(1)
            .mount(&server)
            .await;
        let z = Ziel::aus(None, Some(&server.uri()));
        let l = modelle_laden(&z, Some(&Secret::new(KEY))).await;
        assert_eq!(l.modelle, ["claude-opus-5-5"]);
        // Nie ein Bearer-Header beim direkten Aufruf.
        let anfragen = server.received_requests().await.unwrap();
        assert!(anfragen[0].headers.get("authorization").is_none());
        assert_eq!(anfragen[0].url.query(), Some("limit=1000"));
    }

    #[tokio::test]
    async fn check_reports_reachable_key_and_model() {
        let server = proxy(
            ResponseTemplate::new(200)
                .set_body_json(modelle(&["claude-sonnet-5-5", "claude-haiku-5-5"])),
        )
        .await;
        let p = pruefen(
            &ziel(&server),
            Some(&Secret::new(KEY)),
            Some("claude-sonnet-5-5"),
        )
        .await;
        assert!(p.konfiguriert && p.erreichbar && p.key_ok && p.modell_vorhanden);
        assert_eq!(p.hinweis, None);
    }

    #[tokio::test]
    async fn missing_model_names_the_offered_ones() {
        let server =
            proxy(ResponseTemplate::new(200).set_body_json(modelle(&["gpt-x", "claude-a"]))).await;
        let z = Ziel::aus(Some(&format!("{}/", server.uri())), None);
        let p = pruefen(&z, Some(&Secret::new(KEY)), Some("claude-sonnet-5-5")).await;
        assert!(p.erreichbar && p.key_ok && !p.modell_vorhanden);
        let hinweis = p.hinweis.unwrap();
        assert!(
            hinweis.contains("gpt-x") && hinweis.contains("claude-a"),
            "{hinweis}"
        );
    }

    #[tokio::test]
    async fn check_without_a_model_gives_a_hint() {
        let server = proxy(ResponseTemplate::new(200).set_body_json(modelle(&["a"]))).await;
        let p = pruefen(&ziel(&server), Some(&Secret::new(KEY)), None).await;
        assert!(p.key_ok && !p.modell_vorhanden);
        assert!(p.hinweis.unwrap().contains("Kein Modell"));
    }

    #[tokio::test]
    async fn check_with_a_wrong_key_is_reachable_but_not_ok() {
        let server = proxy(ResponseTemplate::new(401)).await;
        let p = pruefen(&ziel(&server), Some(&Secret::new(KEY)), Some("m")).await;
        assert!(p.erreichbar && !p.key_ok && !p.modell_vorhanden);
        assert!(p.hinweis.unwrap().contains("Key"));
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let ziel_server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(modelle(&["m"])))
            .expect(0)
            .mount(&ziel_server)
            .await;
        let server = proxy_n(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/v1/models", ziel_server.uri())),
            2,
        )
        .await;
        let p = pruefen(&ziel(&server), Some(&Secret::new(KEY)), Some("m")).await;
        assert!(p.erreichbar && !p.key_ok);
        let l = modelle_laden(&ziel(&server), Some(&Secret::new(KEY))).await;
        assert!(!l.geladen);
    }

    #[tokio::test]
    async fn unreachable_endpoint() {
        // Ein Port, auf dem niemand lauscht.
        let frei = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let z = Ziel::aus(Some(&format!("http://127.0.0.1:{frei}")), None);
        let p = pruefen(&z, Some(&Secret::new(KEY)), Some("m")).await;
        assert!(p.konfiguriert && !p.erreichbar && !p.key_ok);
        assert!(p.hinweis.unwrap().contains("nicht erreichbar"));
        let l = modelle_laden(&z, Some(&Secret::new(KEY))).await;
        assert!(!l.erreichbar && !l.geladen);
    }

    #[tokio::test]
    async fn without_a_key_nothing_is_called_and_the_key_never_shows_up() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let p = pruefen(&ziel(&server), None, Some("m")).await;
        assert!(p.konfiguriert && !p.erreichbar);
        assert!(p.hinweis.as_deref().unwrap().contains("agent-key"));
        let l = modelle_laden(&ziel(&server), Some(&Secret::new(""))).await;
        assert!(!l.geladen && l.hinweis.unwrap().contains("kein Key"));

        let server = proxy(ResponseTemplate::new(401)).await;
        let p = pruefen(&ziel(&server), Some(&Secret::new(KEY)), Some("m")).await;
        let l = modelle_laden(&ziel(&server_zweit().await), Some(&Secret::new(KEY))).await;
        let text = format!(
            "{}{}",
            serde_json::to_string(&p).unwrap(),
            serde_json::to_string(&l).unwrap()
        );
        assert!(!text.contains(KEY) && !text.contains("geheim"), "{text}");
    }

    async fn server_zweit() -> MockServer {
        proxy(ResponseTemplate::new(500)).await
    }
}
