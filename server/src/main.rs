//! Cockpit-Backend: lokaler Dienst für eine Person.
//!
//! * lauscht nur auf `127.0.0.1`
//! * liefert die gebaute Web-App aus und beantwortet `/api/…`
//! * hält Daten (SQLite) und Schlüssel (Schlüsselbund) – Schlüssel verlassen das Backend nie
//!
//! ```text
//! cockpit-server                                 starten
//! cockpit-server token setzen <github|jira>      Schlüssel von der Standardeingabe ablegen
//! cockpit-server token loeschen <github|jira>    Schlüssel entfernen
//! ```

mod abgleich;
mod app;
mod config;
mod db;
mod live;
mod secrets;
mod zugang;

use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::ExitCode;
use std::sync::Arc;

use cockpit_integrations::Secret;
use config::Config;
use secrets::{KeyringStore, SecretStore};

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cockpit_server=info,tower_http=warn".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let ergebnis = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => starten().await,
        ["token", aktion, name] => token(aktion, name),
        _ => Err("Aufruf: cockpit-server [token setzen|loeschen <github|jira>]".into()),
    };
    match ergebnis {
        Ok(()) => ExitCode::SUCCESS,
        Err(m) => {
            tracing::error!("{m}");
            ExitCode::FAILURE
        }
    }
}

/// Legt einen Schlüssel ab oder löscht ihn. Der Wert kommt von der Standardeingabe, damit er
/// weder in der Prozessliste noch in der Shell-Historie landet.
fn token(aktion: &str, name: &str) -> Result<(), String> {
    if ![secrets::GITHUB, secrets::JIRA].contains(&name) {
        return Err(format!(
            "Unbekannter Name „{name}“, erlaubt sind github und jira"
        ));
    }
    let store = KeyringStore;
    match aktion {
        "setzen" => {
            let mut eingabe = String::new();
            std::io::stdin()
                .read_to_string(&mut eingabe)
                .map_err(|e| format!("Eingabe nicht lesbar: {e}"))?;
            let wert = eingabe.trim();
            if wert.is_empty() {
                return Err("Kein Schlüssel auf der Standardeingabe".into());
            }
            store
                .set(name, &Secret::new(wert))
                .map_err(|e| e.to_string())?;
            tracing::info!("Schlüssel für {name} abgelegt");
            Ok(())
        }
        "loeschen" => {
            store.delete(name).map_err(|e| e.to_string())?;
            tracing::info!("Schlüssel für {name} gelöscht");
            Ok(())
        }
        _ => Err(format!(
            "Unbekannte Aktion „{aktion}“, erlaubt sind setzen und loeschen"
        )),
    }
}

async fn starten() -> Result<(), String> {
    let config = Config::laden().map_err(|e| e.to_string())?;

    let conn = db::open(&config.db).map_err(|e| e.to_string())?;
    tracing::info!(
        "Datenbank: {} (Version {})",
        config.db.display(),
        db::AKTUELLE_VERSION
    );
    // Adressen und Konten aus der Konfiguration in die Datenbank übernehmen (ohne Schlüssel).
    if let Some(g) = &config.github {
        db::verbindung_setzen(&conn, secrets::GITHUB, &g.api_url, None)
            .map_err(|e| e.to_string())?;
    }
    if let Some(j) = &config.jira {
        db::verbindung_setzen(&conn, secrets::JIRA, &j.site, Some(&j.email))
            .map_err(|e| e.to_string())?;
    }

    if !config.dist.join("index.html").is_file() {
        tracing::warn!(
            "Keine Web-App unter {} – zuerst `cd cockpit && trunk build`, oder COCKPIT_DIST setzen",
            config.dist.display()
        );
    }

    let state = app::AppState::mit_quellen(
        conn,
        Arc::new(KeyringStore),
        config.port,
        abgleich::Quellen::aus(&config),
    );
    let router = app::router(state.clone(), config.dist.clone());

    // Nur lokal erreichbar – bewusst nicht einstellbar.
    let adresse = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let listener = tokio::net::TcpListener::bind(adresse)
        .await
        .map_err(|e| format!("Port {} nicht verfügbar: {e}", config.port))?;
    tracing::info!("Cockpit läuft auf http://{adresse}");

    // Im Server-Modus läuft der Takt des simulierten Agenten hier, nicht im Browser.
    app::agent_takt_starten(state.clone());
    // Jira und GitHub: regelmäßig abgleichen. Ohne Quelle oder Schlüssel passiert nichts.
    abgleich::abgleich_starten(state);

    // Die Adresse der Gegenstelle braucht `zugang`, um Fremde abzuweisen.
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Beende …");
    })
    .await
    .map_err(|e| format!("Server beendet mit Fehler: {e}"))
}
