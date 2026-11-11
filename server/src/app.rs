//! Der HTTP-Teil: Routen unter `/api`, dahinter die gebaute Web-App.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{any, get};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::Serialize;
use tower_http::services::ServeDir;

use crate::db;
use crate::secrets::{self, SecretStore};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Gemeinsamer Zustand der Handler. Klonen ist billig.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    db: Mutex<Connection>,
    secrets: Arc<dyn SecretStore>,
}

impl AppState {
    pub fn new(db: Connection, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            inner: Arc::new(Inner {
                db: Mutex::new(db),
                secrets,
            }),
        }
    }
}

/// Ob eine Verbindung eingerichtet ist: Sie steht in der Datenbank **und** ihr Schlüssel
/// liegt in der Schlüsselablage. Der Schlüssel selbst wird nie gelesen oder ausgegeben.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Verbindungen {
    pub github: bool,
    pub jira: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
    pub verbindungen: Verbindungen,
}

impl AppState {
    /// Blockierend (Datenbank, Schlüsselbund) – nicht direkt in einem async-Handler aufrufen.
    fn health(&self) -> Health {
        let namen = match self.inner.db.lock() {
            Ok(conn) => db::verbindungen(&conn).unwrap_or_else(|e| {
                tracing::warn!("Verbindungen konnten nicht gelesen werden: {e}");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        let eingerichtet = |name: &str| {
            namen.iter().any(|n| n == name)
                && self.inner.secrets.has(name).unwrap_or_else(|e| {
                    tracing::warn!("Schlüssel „{name}“ nicht prüfbar: {e}");
                    false
                })
        };
        Health {
            status: "ok",
            version: VERSION,
            verbindungen: Verbindungen {
                github: eingerichtet(secrets::GITHUB),
                jira: eingerichtet(secrets::JIRA),
            },
        }
    }
}

async fn health(State(state): State<AppState>) -> Result<Json<Health>, StatusCode> {
    tokio::task::spawn_blocking(move || state.health())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn api_unbekannt() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "fehler": "Unbekannte Adresse" })),
    )
}

/// Baut den Router. `dist` ist der Ordner der gebauten Web-App.
pub fn router(state: AppState, dist: PathBuf) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/{*rest}", any(api_unbekannt))
        .with_state(state)
        .fallback_service(ServeDir::new(dist))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use cockpit_integrations::Secret;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::secrets::MemoryStore;

    fn zustand(secrets: Arc<MemoryStore>) -> AppState {
        let conn = db::open_im_speicher().unwrap();
        db::verbindung_setzen(&conn, "github", "https://api.github.com", None).unwrap();
        db::verbindung_setzen(&conn, "jira", "https://x.atlassian.net", Some("a@b.de")).unwrap();
        AppState::new(conn, secrets)
    }

    async fn hole(app: Router, pfad: &str) -> (StatusCode, String) {
        let antwort = app
            .oneshot(Request::get(pfad).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = antwort.status();
        let body = antwort.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn health_ohne_schluessel() {
        let app = router(
            zustand(Arc::new(MemoryStore::new())),
            PathBuf::from("/gibt/es/nicht"),
        );
        let (status, body) = hole(app, "/api/health").await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], VERSION);
        assert_eq!(json["verbindungen"]["github"], false);
        assert_eq!(json["verbindungen"]["jira"], false);
    }

    #[tokio::test]
    async fn health_zeigt_nur_ja_nein_nie_den_schluessel() {
        let secrets = Arc::new(MemoryStore::new());
        secrets
            .set(secrets::GITHUB, &Secret::new("ghp_streng_geheim"))
            .unwrap();
        let app = router(zustand(secrets), PathBuf::from("/gibt/es/nicht"));
        let (status, body) = hole(app, "/api/health").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("ghp_streng_geheim"), "{body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["verbindungen"]["github"], true);
        assert_eq!(json["verbindungen"]["jira"], false);
    }

    #[tokio::test]
    async fn schluessel_ohne_verbindung_zaehlt_nicht() {
        let secrets = Arc::new(MemoryStore::new());
        secrets.set(secrets::JIRA, &Secret::new("t")).unwrap();
        let state = AppState::new(db::open_im_speicher().unwrap(), secrets);
        let h = state.health();
        assert_eq!(
            h.verbindungen,
            Verbindungen {
                github: false,
                jira: false
            }
        );
    }

    #[tokio::test]
    async fn unbekannte_api_adresse_ist_404_json() {
        let app = router(
            zustand(Arc::new(MemoryStore::new())),
            PathBuf::from("/gibt/es/nicht"),
        );
        let (status, body) = hole(app, "/api/gibt-es-nicht").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.contains("fehler"));
    }

    #[tokio::test]
    async fn liefert_die_app_als_statische_dateien() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("index.html"), "<h1>Cockpit</h1>").unwrap();
        std::fs::write(tmp.path().join("app.js"), "console.log(1)").unwrap();
        let app = || {
            router(
                zustand(Arc::new(MemoryStore::new())),
                tmp.path().to_path_buf(),
            )
        };

        let (status, body) = hole(app(), "/").await;
        assert_eq!(
            (status, body.as_str()),
            (StatusCode::OK, "<h1>Cockpit</h1>")
        );
        let (status, body) = hole(app(), "/app.js").await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "console.log(1)"));
        let (status, _) = hole(app(), "/fehlt.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
