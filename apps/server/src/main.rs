//! Cockpit backend: local service for one person.
//!
//! * listens only on `127.0.0.1` (the container image sets `COCKPIT_BIND=0.0.0.0`, see docs/container.md)
//! * serves the built web app and answers `/api/…`
//! * holds data (SQLite) and keys (`.env` file in the project folder) – keys never leave the backend
//!
//! ```text
//! cockpit-server                                 start
//! cockpit-server token set <name>                store a key from standard input
//! cockpit-server token delete <name>             remove a key
//! ```
//!
//! `<name>`: `github`, `jira` (API token), `github-oauth` and `atlassian-oauth` (client secret
//! of the OAuth apps) as well as `agent-key` (key of the agent endpoint). The tokens of the sign-in are stored by the backend itself.

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
mod projects_write;
mod review_panel;
mod secrets;
mod setup;
mod sync;
mod webhooks;

use std::io::Read;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use cockpit_integrations::Secret;
use config::Config;
use secrets::{EnvFileStore, SecretStore};

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
        ["health"] => health(),
        ["token", action, name] => token(action, name),
        _ => Err(
            "Usage: cockpit-server [health] | [token set|delete <github|jira|github-oauth|atlassian-oauth|agent-key>]"
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

/// `cockpit-server health`: asks the running server on `127.0.0.1` for `/api/health` and succeeds on a
/// `200` answer. This is the healthcheck of the container image (no `curl` in the image).
fn health() -> Result<(), String> {
    use std::io::Write;
    use std::time::Duration;
    let port = std::env::var("COCKPIT_PORT")
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
        .unwrap_or(config::DEFAULT_PORT);
    let address = SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
    let mut stream = std::net::TcpStream::connect_timeout(&address, Duration::from_secs(3))
        .map_err(|e| format!("Server not reachable on {address}: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let request =
        format!("GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("Request failed: {e}"))?;
    let mut answer = String::new();
    let mut first = [0u8; 64];
    let n = stream
        .read(&mut first)
        .map_err(|e| format!("No answer: {e}"))?;
    answer.push_str(&String::from_utf8_lossy(&first[..n]));
    if answer.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        Err(format!(
            "Unexpected answer: {}",
            answer.lines().next().unwrap_or("")
        ))
    }
}

/// Names that can be set via `token`.
const NAMES: &[&str] = &[
    secrets::GITHUB,
    secrets::JIRA,
    secrets::GITHUB_OAUTH,
    secrets::ATLASSIAN_OAUTH,
    secrets::AGENT_KEY,
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
        "set" => {
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
        "delete" => {
            store.delete(name).map_err(|e| e.to_string())?;
            tracing::info!("Key for {name} deleted");
            Ok(())
        }
        _ => Err(format!(
            "Unknown action \"{action}\", allowed: set and delete"
        )),
    }
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
    let router = app::router(state.clone(), config.dist.clone());

    // Only reachable locally by default. `COCKPIT_BIND` (environment only) is for the container image,
    // where the published port must be `127.0.0.1` on the host (see docs/security.md).
    let address = SocketAddr::from((config.bind, config.port));
    if !config.bind.is_loopback() {
        tracing::warn!(
            "Listening on {address} (COCKPIT_BIND): publish the port on 127.0.0.1 of the host only"
        );
    }
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|e| format!("Port {} not available: {e}", config.port))?;
    tracing::info!("Cockpit running at http://{address}");

    // The watcher of the agent containers and the tick of the other background tasks.
    agent_ops::watcher_start(state.clone());
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
