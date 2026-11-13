//! Cockpit backend: local service for one person.
//!
//! * listens only on `127.0.0.1`
//! * serves the built web app and answers `/api/…`
//! * holds data (SQLite) and keys (keychain) – keys never leave the backend
//!
//! ```text
//! cockpit-server                                 start
//! cockpit-server token setzen <name>             store a key from standard input
//! cockpit-server token loeschen <name>           remove a key
//! ```
//!
//! `<name>`: `github`, `jira` (API token), `github-oauth` and `atlassian-oauth` (client secret
//! of the OAuth apps) as well as `agent-key` (key of the agent endpoint; `litellm` counts as an older name). The tokens of the sign-in are stored by the backend itself.
//!
//! ```text
//! ```

mod access;
mod agent_git;
mod agent_ops;
mod agent_run;
mod app;
mod config;
mod container;
mod db;
mod live;
mod oauth;
mod secrets;
mod setup;
mod sync;

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
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => start().await,
        ["token", action, name] => token(action, name),
        _ => Err("Aufruf: cockpit-server [token setzen|loeschen <github|jira|github-oauth|atlassian-oauth|agent-key|litellm>]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(m) => {
            tracing::error!("{m}");
            ExitCode::FAILURE
        }
    }
}

/// Names that can be set via `token`.
const NAMES: &[&str] = &[
    secrets::GITHUB,
    secrets::JIRA,
    secrets::GITHUB_OAUTH,
    secrets::ATLASSIAN_OAUTH,
    secrets::AGENT_KEY,
    secrets::LITELLM,
];

/// Stores a key or deletes it. The value comes from standard input so that it ends up
/// neither in the process list nor in the shell history.
fn token(action: &str, name: &str) -> Result<(), String> {
    if !NAMES.contains(&name) {
        return Err(format!(
            "Unbekannter Name „{name}“, erlaubt sind {}",
            NAMES.join(", ")
        ));
    }
    let store = KeyringStore;
    match action {
        "setzen" => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| format!("Eingabe nicht lesbar: {e}"))?;
            let wert = input.trim();
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
            "Unbekannte Aktion „{action}“, erlaubt sind setzen und loeschen"
        )),
    }
}

async fn start() -> Result<(), String> {
    let config = Config::laden().map_err(|e| e.to_string())?;

    let conn = db::open(&config.db).map_err(|e| e.to_string())?;
    tracing::info!(
        "Datenbank: {} (Version {})",
        config.db.display(),
        db::CURRENT_VERSION
    );

    if !config.dist.join("index.html").is_file() {
        tracing::warn!(
            "Keine Web-App unter {} – zuerst `cd cockpit && trunk build`, oder COCKPIT_DIST setzen",
            config.dist.display()
        );
    }

    let state = app::AppState::create(
        conn,
        Arc::new(KeyringStore),
        config.port,
        sync::Sources::from_config(&config),
        app::Settings::from_config(&config),
    );
    // Take over addresses and accounts from the configuration into the database (without keys).
    state.connections_adopt(&config)?;
    let router = app::router(state.clone(), config.dist.clone());

    // Only reachable locally – deliberately not configurable.
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|e| format!("Port {} nicht verfügbar: {e}", config.port))?;
    tracing::info!("Cockpit läuft auf http://{address}");

    // In server mode the tick of the simulated agent runs here, not in the browser.
    app::agent_takt_start(state.clone());
    // Jira and GitHub: sync regularly. Without a source or key nothing happens.
    sync::sync_start(state);

    // The address of the peer is needed by `access` to turn away strangers.
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
