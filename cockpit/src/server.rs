//! Der Server-Modus aus Sicht des Browsers: Anfragen an das lokale Backend
//! (`cockpit-server`) und der Ereignisstrom. Die Formen der Daten stehen in
//! `cockpit_core::api`. Hier steht kein Schlüssel und es wird keiner gespeichert.

use cockpit_core::ServiceStatus;
use cockpit_core::api::{
    ActionRequest, ActionResponse, ContainerStatus, Einrichtung, ErrorBody, JiraProjectsResponse,
    KonfigAenderung, KonfigAntwort, LitellmPruefung, LitellmStatus, SchluesselAntwort,
    SchluesselEingabe, ServerEvent, SyncResponse, WiderrufAntwort,
};
use cockpit_core::{Action, Workspace, persist};
use gloo_net::http::Request;
use web_sys::wasm_bindgen::{JsCast, closure::Closure};

/// Warum eine Anfrage an den Server nichts ergab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// Der Server hat die Anfrage gesehen und abgelehnt – mit Meldung für die Oberfläche.
    Rejected(String),
    /// Keine Antwort: Der Server läuft nicht oder ist nicht unter dieser Adresse.
    Unreachable,
}

impl CallError {
    pub fn message(&self) -> String {
        match self {
            CallError::Rejected(m) => m.clone(),
            CallError::Unreachable => "Der Server ist nicht erreichbar".into(),
        }
    }
}

/// Die Meldung aus einer Fehlerantwort, sonst eine allgemeine.
async fn rejection(resp: gloo_net::http::Response) -> CallError {
    let status = resp.status();
    let message = resp
        .json::<ErrorBody>()
        .await
        .map(|e| e.fehler)
        .unwrap_or_else(|_| format!("Der Server antwortet mit Fehler {status}"));
    CallError::Rejected(message)
}

/// Lädt den aktuellen Stand samt Revision.
pub async fn fetch_workspace() -> Result<(Workspace, u64), CallError> {
    let resp = Request::get("/api/workspace")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    let text = resp.text().await.map_err(|_| CallError::Unreachable)?;
    persist::load_with_revision(&text)
        .map_err(|_| CallError::Rejected("Der Server schickt einen unlesbaren Stand".into()))
}

/// Schickt eine Aktion. Der Server wendet sie als „Du“ an.
pub async fn post_action(action: Action) -> Result<ActionResponse, CallError> {
    let request = Request::post("/api/actions")
        .header("Content-Type", "application/json")
        .json(&ActionRequest { action })
        .map_err(|_| CallError::Rejected("Die Aktion lässt sich nicht senden".into()))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<ActionResponse>()
        .await
        .map_err(|_| CallError::Rejected("Der Server antwortet unverständlich".into()))
}

/// „Jetzt abgleichen“: stößt den Abgleich mit Jira und GitHub an (`POST /api/sync`).
pub async fn post_sync() -> Result<SyncResponse, CallError> {
    let request = Request::post("/api/sync")
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected("Der Abgleich lässt sich nicht starten".into()))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<SyncResponse>()
        .await
        .map_err(|_| CallError::Rejected("Der Server antwortet unverständlich".into()))
}

/// Die Jira-Projekte für die Auswahl in der Inbox (`GET /api/jira/projekte`).
pub async fn fetch_jira_projects() -> Result<JiraProjectsResponse, CallError> {
    let resp = Request::get("/api/jira/projekte")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<JiraProjectsResponse>()
        .await
        .map_err(|_| CallError::Rejected("Der Server antwortet unverständlich".into()))
}

/// Offener Ereignisstrom. Beim Schließen (`Drop`) wird die Verbindung beendet.
pub struct Events {
    source: web_sys::EventSource,
}

impl Drop for Events {
    fn drop(&mut self) {
        self.source.close();
    }
}

impl Events {
    /// Öffnet `GET /api/events`. Der Browser verbindet nach einem Abbruch selbst neu; dann
    /// kommt wieder `on_open`.
    pub fn open(
        on_open: impl Fn() + 'static,
        on_event: impl Fn(ServerEvent) + 'static,
        on_error: impl Fn() + 'static,
    ) -> Option<Self> {
        let source = web_sys::EventSource::new("/api/events").ok()?;
        let open = Closure::<dyn Fn()>::new(on_open);
        let error = Closure::<dyn Fn()>::new(on_error);
        let message =
            Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                let parsed = ev
                    .data()
                    .as_string()
                    .and_then(|text| serde_json::from_str::<ServerEvent>(&text).ok());
                if let Some(event) = parsed {
                    on_event(event);
                }
            });
        source.set_onopen(Some(open.as_ref().unchecked_ref()));
        source.set_onerror(Some(error.as_ref().unchecked_ref()));
        source.set_onmessage(Some(message.as_ref().unchecked_ref()));
        // Die Rückrufe leben, solange die Seite offen ist.
        open.forget();
        error.forget();
        message.forget();
        Some(Self { source })
    }
}

/// Wie `GET /api/health` die Anmeldung je Dienst meldet.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Services {
    #[serde(default)]
    pub github: ServiceStatus,
    #[serde(default)]
    pub jira: ServiceStatus,
}

/// Der Teil von `GET /api/health`, den die Seite „Einrichten“ braucht. Nie ein Schlüssel:
/// Der Server meldet nur ja/nein.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Health {
    #[serde(default)]
    pub anmeldung: Services,
    #[serde(default)]
    pub litellm: LitellmStatus,
    #[serde(default)]
    pub container: ContainerStatus,
}

/// Stand der Anmeldung, des LiteLLM-Proxys und der Container-Laufzeit (`GET /api/health`).
pub async fn fetch_health() -> Result<Health, CallError> {
    let resp = Request::get("/api/health")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<Health>()
        .await
        .map_err(|_| CallError::Rejected("Der Server antwortet unverständlich".into()))
}

/// Art und Stand der Anmeldung bei GitHub und Jira (`GET /api/health`).
pub async fn fetch_services() -> Result<Services, CallError> {
    fetch_health().await.map(|h| h.anmeldung)
}

/// Verbindungstest zum LiteLLM-Proxy (`GET /api/litellm/pruefen`).
pub async fn fetch_litellm_check() -> Result<LitellmPruefung, CallError> {
    let resp = Request::get("/api/litellm/pruefen")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<LitellmPruefung>()
        .await
        .map_err(|_| CallError::Rejected("Der Server antwortet unverständlich".into()))
}

/// „Trennen“: widerruft die Anmeldung (`POST /api/oauth/{dienst}/widerrufen`). `service`:
/// `github` oder `atlassian`.
pub async fn post_revoke(service: &str) -> Result<WiderrufAntwort, CallError> {
    let request = Request::post(&format!("/api/oauth/{service}/widerrufen"))
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected("Das Trennen lässt sich nicht starten".into()))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<WiderrufAntwort>()
        .await
        .map_err(|_| CallError::Rejected("Der Server antwortet unverständlich".into()))
}

/// Die aktuellen, nicht geheimen Werte für den Assistenten (`GET /api/einrichtung`).
pub async fn fetch_einrichtung() -> Result<Einrichtung, CallError> {
    let resp = Request::get("/api/einrichtung")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<Einrichtung>()
        .await
        .map_err(|_| CallError::Rejected("Der Server antwortet unverständlich".into()))
}

/// Schreibt Client-IDs, Jira, Repos oder LiteLLM in `config.toml` (`PUT /api/konfiguration`).
pub async fn put_konfiguration(aenderung: &KonfigAenderung) -> Result<KonfigAntwort, CallError> {
    let request = Request::put("/api/konfiguration")
        .header("Content-Type", "application/json")
        .json(aenderung)
        .map_err(|_| CallError::Rejected("Die Eingaben lassen sich nicht senden".into()))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<KonfigAntwort>()
        .await
        .map_err(|_| CallError::Rejected("Der Server antwortet unverständlich".into()))
}

/// Legt einen Schlüssel im Schlüsselbund des Servers ab (`PUT /api/schluessel/{name}`). Der Wert
/// geht nur in den Body dieser Anfrage – nicht in die Adresse, nicht in den Speicher des Browsers.
/// Die Antwort sagt nur „gesetzt“.
pub async fn put_schluessel(name: &str, wert: String) -> Result<SchluesselAntwort, CallError> {
    let request = Request::put(&format!("/api/schluessel/{name}"))
        .header("Content-Type", "application/json")
        .json(&SchluesselEingabe { wert })
        .map_err(|_| CallError::Rejected("Die Eingabe lässt sich nicht senden".into()))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<SchluesselAntwort>()
        .await
        .map_err(|_| CallError::Rejected("Der Server antwortet unverständlich".into()))
}
