//! Cockpit backend: local service for one person.
//!
//! * listens only on `127.0.0.1`
//! * serves the built web app and answers `/api/…`
//! * holds data (SQLite) and keys (`.env` file in the project folder) – keys never leave the backend
//!
//! ```text
//! cockpit-server                                 start
//! cockpit-server token set <name>                store a key from standard input
//! cockpit-server token delete <name>             remove a key
//! cockpit-server token import                    copy old keychain entries into the .env file
//! ```
//!
//! The older German spellings `setzen`, `loeschen` and `uebernehmen` are still accepted.
//!
//! `<name>`: `github`, `jira` (API token), `github-oauth` and `atlassian-oauth` (client secret
//! of the OAuth apps) as well as `agent-key` (key of the agent endpoint; `litellm` counts as an older name). The tokens of the sign-in are stored by the backend itself.

mod access;
mod agent_followup;
mod agent_git;
mod agent_image;
mod agent_ops;
mod agent_run;
mod app;
mod attachments;
mod config;
mod container;
mod db;
mod live;
mod oauth;
mod review_panel;
mod secrets;
mod setup;
mod sync;
mod translate;
mod webhooks;

use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::ExitCode;
use std::sync::Arc;

use cockpit_integrations::Secret;
use config::Config;
use secrets::{EnvFileStore, KeyringStore, SecretStore};

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
        ["token", "import" | "uebernehmen"] => take_over_keychain(),
        _ => Err(
            "Usage: cockpit-server [token set|delete <github|jira|github-oauth|atlassian-oauth|agent-key>] | [token import]"
                .into(),
        ),
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
            "Unknown name \"{name}\", allowed: {}",
            NAMES.join(", ")
        ));
    }
    let store = EnvFileStore::from_environment();
    match action {
        "set" | "setzen" => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| format!("Could not read standard input: {e}"))?;
            let value = input.trim();
            if value.is_empty() {
                return Err("No key on standard input".into());
            }
            store
                .set(name, &Secret::new(value))
                .map_err(|e| e.to_string())?;
            tracing::info!("Key for {name} stored");
            Ok(())
        }
        "delete" | "loeschen" => {
            store.delete(name).map_err(|e| e.to_string())?;
            tracing::info!("Key for {name} deleted");
            Ok(())
        }
        _ => Err(format!(
            "Unknown action \"{action}\", allowed: set and delete"
        )),
    }
}

/// Copies keys from the system keychain into the `.env` file. Only on explicit call
/// (`cockpit-server token import`): reads the keychain, deletes nothing there and does not
/// overwrite anything in the file that already has a value.
fn take_over_keychain() -> Result<(), String> {
    let target = EnvFileStore::from_environment();
    let report = secrets::take_over(&KeyringStore, &target).map_err(|e| e.to_string())?;
    for (name, outcome) in report {
        let text = match outcome {
            secrets::Takeover::Taken => "imported",
            secrets::Takeover::AlreadyPresent => "already has a value, left unchanged",
            secrets::Takeover::NotFound => "not in the keychain",
        };
        tracing::info!("{name}: {text}");
    }
    tracing::info!(
        "Done. The keychain entries remain; the file is {}",
        target.path().display()
    );
    Ok(())
}

async fn start() -> Result<(), String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let store = Arc::new(EnvFileStore::from_environment());
    tracing::info!("Keys are stored in the file {}", store.path().display());
    for warning in store.check_at_startup() {
        tracing::warn!("{warning}");
    }

    if let Some(dir) = &config.config_dir {
        for note in config::files_check::check_and_fix(dir) {
            tracing::warn!("{note}");
        }
    }
    let conn = db::open(&config.db).map_err(|e| e.to_string())?;
    config::location::tighten_db(&config.db);
    tracing::info!(
        "Database: {} (version {})",
        config.db.display(),
        db::CURRENT_VERSION
    );

    if !config.dist.join("index.html").is_file() {
        tracing::warn!(
            "No web app at {} - first run `cd apps/cockpit && trunk build`, or set COCKPIT_DIST",
            config.dist.display()
        );
    }

    let state = app::AppState::create(
        conn,
        store,
        config.port,
        sync::Sources::from_config(&config),
        app::Settings::from_config(&config),
    );
    // Take over addresses and accounts from the configuration into the database (without keys).
    state
        .connections_adopt(&config)
        .map_err(|m| m.render(cockpit_i18n::Lang::En))?;
    let router = app::router(state.clone(), config.dist.clone());

    // Only reachable locally – deliberately not configurable.
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|e| format!("Port {} not available: {e}", config.port))?;
    tracing::info!("Cockpit running at http://{address}");

    // In server mode the tick of the simulated agent runs here, not in the browser.
    app::agent_tick_start(state.clone());
    // Jira and GitHub: sync regularly. Without a source or key nothing happens.
    sync::sync_start(state);

    // The address of the peer is needed by `access` to turn away strangers.
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Shutting down ...");
    })
    .await
    .map_err(|e| format!("Server exited with error: {e}"))
}
