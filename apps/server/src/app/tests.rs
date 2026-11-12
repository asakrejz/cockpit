use super::*;
use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use cockpit_core::Actor;
use cockpit_integrations::Secret;
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::config::EndpunktKonfig;
use crate::secrets::MemoryStore;
use cockpit_core::api::Provider;

fn zustand(secrets: Arc<MemoryStore>) -> AppState {
    let conn = db::open_im_speicher().unwrap();
    db::verbindung_setzen(&conn, "github", "https://api.github.com", None).unwrap();
    db::verbindung_setzen(&conn, "jira", "https://x.atlassian.net", Some("a@b.de")).unwrap();
    AppState::new(conn, secrets)
}

/// Eine Anfrage, wie sie der Browser von `127.0.0.1:8787` schickt.
fn anfrage(methode: &str, pfad: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(methode)
        .uri(pfad)
        .header("host", "127.0.0.1:8787")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
}

async fn sende(app: Router, req: Request<Body>) -> (StatusCode, String) {
    let antwort = app.oneshot(req).await.unwrap();
    let status = antwort.status();
    let body = antwort.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn hole(app: Router, pfad: &str) -> (StatusCode, String) {
    sende(app, anfrage("GET", pfad).body(Body::empty()).unwrap()).await
}

async fn schicke(app: Router, json: &str) -> (StatusCode, String) {
    let req = anfrage("POST", "/api/actions")
        .header("content-type", "application/json")
        .body(Body::from(json.to_string()))
        .unwrap();
    sende(app, req).await
}

fn app_ohne_dist() -> Router {
    router(
        zustand(Arc::new(MemoryStore::new())),
        PathBuf::from("/gibt/es/nicht"),
    )
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap()
}

/// Interne ID des ersten Eintrags der Beispieldaten.
fn erste_id() -> String {
    cockpit_core::demo::workspace().items[0].id.clone()
}

fn mark_done(id: &str) -> String {
    format!(r#"{{"action":{{"MarkDone":{{"item":"{id}"}}}}}}"#)
}

/// Liest vom Ereignisstrom, bis `n` Ereignisse (`data:`-Zeilen) da sind.
async fn lese_ereignisse(body: Body, n: usize) -> Vec<serde_json::Value> {
    use http_body_util::BodyExt;
    let mut body = body;
    let mut text = String::new();
    let mut daten = Vec::new();
    while daten.len() < n {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("Ereignis kam nicht")
            .expect("Strom zu Ende")
            .unwrap();
        if let Some(chunk) = frame.data_ref() {
            text.push_str(&String::from_utf8_lossy(chunk));
        }
        daten = text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|l| json(l.trim()))
            .collect();
    }
    daten
}

fn zustand_mit(conn: Connection, e: Einstellungen, secrets: Arc<MemoryStore>) -> AppState {
    AppState::neu(conn, secrets, 8787, Quellen::default(), e)
}

fn leer() -> Einstellungen {
    Einstellungen {
        suchpfad: Some(std::ffi::OsString::new()),
        ..Einstellungen::default()
    }
}

async fn stand_von(state: AppState) -> (cockpit_core::Workspace, u64) {
    let app = router(state, PathBuf::from("/x"));
    let (status, body) = hole(app, "/api/workspace").await;
    assert_eq!(status, StatusCode::OK);
    cockpit_core::persist::load_with_revision(&body).unwrap()
}

/// `url = None`: direkt beim Anbieter, dann geht der Aufruf an `anthropic_api` (die Attrappe).
fn mit_endpunkt(
    url: Option<&str>,
    anthropic_api: Option<&str>,
    secrets: Arc<MemoryStore>,
) -> Router {
    let e = Einstellungen {
        agent: AgentWahl {
            anbieter: Provider::ClaudeAgentSdk,
            endpunkt: Some(EndpunktKonfig {
                url: url.map(str::to_string),
                modell: Some("claude-sonnet-5-5".into()),
            }),
        },
        anthropic_api: anthropic_api.map(str::to_string),
        ..leer()
    };
    let conn = db::open_im_speicher().unwrap();
    router(zustand_mit(conn, e, secrets), PathBuf::from("/x"))
}

mod actions;
mod basics;
mod endpoint;
mod events;
mod persistence;
mod security;
mod seed;
mod tick;
