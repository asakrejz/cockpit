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

// ---------- GET /api/workspace ----------

#[tokio::test]
async fn workspace_startet_mit_den_demo_daten() {
    let (status, body) = hole(app_ohne_dist(), "/api/workspace").await;
    assert_eq!(status, StatusCode::OK);
    let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(ws, cockpit_core::demo::workspace());
    assert_eq!(revision, 0);
}

// ---------- POST /api/actions ----------

#[tokio::test]
async fn aktion_aendert_den_stand_und_zaehlt_die_revision() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let id = erste_id();

    let (status, body) = schicke(app(), &mark_done(&id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let antwort = json(&body);
    assert_eq!(antwort["revision"], 1);
    assert_eq!(antwort["protokolliert"], 1);
    assert!(!antwort["zusammenfassung"].as_str().unwrap().is_empty());

    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(revision, 1);
    assert!(ws.item(&id).unwrap().read);
    // Der Server wendet als „Du“ an, mit seiner Uhr.
    let letzter = ws.log.last().unwrap();
    assert_eq!(letzter.actor, Actor::User);
    assert!(letzter.at_ms > 1_600_000_000_000);
}

#[tokio::test]
async fn regelverstoss_ist_422_mit_deutscher_meldung_und_aendert_nichts() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let vorher = hole(app(), "/api/workspace").await.1;

    let (status, body) =
        schicke(app(), r#"{"action":{"MarkDone":{"item":"gibt-es-nicht"}}}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json(&body)["fehler"]
            .as_str()
            .unwrap()
            .contains("gibt es nicht")
    );
    assert_eq!(hole(app(), "/api/workspace").await.1, vorher);
}

#[tokio::test]
async fn review_senden_mit_offenen_vorschlaegen_ist_4xx() {
    let ws = cockpit_core::demo::workspace();
    let number = ws
        .reviews
        .iter()
        .find(|r| r.suggestions.iter().any(|s| s.decision.is_none()))
        .expect("Beispieldaten haben ein Review mit offenen Vorschlägen")
        .number;
    let body = format!(r#"{{"action":{{"SendReview":{{"review":{number},"comment":""}}}}}}"#);
    let (status, antwort) = schicke(app_ohne_dist(), &body).await;
    assert!(status.is_client_error(), "{status}");
    assert!(
        json(&antwort)["fehler"]
            .as_str()
            .is_some_and(|m| !m.is_empty())
    );
}

#[tokio::test]
async fn mergen_nur_mit_erfuellten_bedingungen() {
    // Über die API kommt jede Aktion als „Du“. Dass ein Agent nie merged, prüft
    // `Workspace::apply` selbst (Actor); hier greifen die Bedingungen fürs Mergen.
    let body = r#"{"action":{"MergePr":{"review":87,"method":"Squash"}}}"#;
    let (status, antwort) = schicke(app_ohne_dist(), body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{antwort}");
    assert!(!json(&antwort)["fehler"].as_str().unwrap().is_empty());
    // Formen, die es nicht gibt, bleiben 400.
    for body in [
        r#"{"action":{"Merge":{"review":87}}}"#,
        r#"{"action":"MergePullRequest"}"#,
    ] {
        let (status, antwort) = schicke(app_ohne_dist(), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            json(&antwort)["fehler"]
                .as_str()
                .unwrap()
                .contains("Aktion")
        );
    }
}

#[tokio::test]
async fn kaputter_body_ist_400() {
    for body in ["", "kein json", "{}", r#"{"action":"GibtEsNicht"}"#] {
        let (status, antwort) = schicke(app_ohne_dist(), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert!(antwort.contains("fehler"));
    }
}

// ---------- Sicherheit ----------

#[tokio::test]
async fn post_ohne_json_content_type_ist_415() {
    for typ in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let mut b = anfrage("POST", "/api/actions");
        if let Some(t) = typ {
            b = b.header("content-type", t);
        }
        let req = b.body(Body::from(mark_done(&erste_id()))).unwrap();
        let (status, _) = sende(app_ohne_dist(), req).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{typ:?}");
    }
}

#[tokio::test]
async fn post_mit_fremder_herkunft_ist_403_und_aendert_nichts() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let vorher = hole(app(), "/api/workspace").await.1;
    let req = anfrage("POST", "/api/actions")
        .header("content-type", "application/json")
        .header("origin", "https://evil.example")
        .body(Body::from(mark_done(&erste_id())))
        .unwrap();
    let (status, _) = sende(app(), req).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(hole(app(), "/api/workspace").await.1, vorher);

    // Gleiche Herkunft geht.
    let req = anfrage("POST", "/api/actions")
        .header("content-type", "application/json")
        .header("origin", "http://localhost:8787")
        .body(Body::from(mark_done(&erste_id())))
        .unwrap();
    assert_eq!(sende(app(), req).await.0, StatusCode::OK);
}

#[tokio::test]
async fn fremder_host_ist_403_auf_allen_api_routen() {
    for (methode, pfad) in [
        ("GET", "/api/health"),
        ("GET", "/api/workspace"),
        ("GET", "/api/events"),
        ("POST", "/api/actions"),
    ] {
        let req = Request::builder()
            .method(methode)
            .uri(pfad)
            .header("host", "evil.example:8787")
            .header("content-type", "application/json")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
            .body(Body::from(mark_done(&erste_id())))
            .unwrap();
        let (status, _) = sende(app_ohne_dist(), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{methode} {pfad}");
    }
}

#[tokio::test]
async fn fremde_gegenstelle_ist_403() {
    let req = Request::get("/api/workspace")
        .header("host", "127.0.0.1:8787")
        .extension(ConnectInfo(SocketAddr::from(([192, 168, 0, 7], 1))))
        .body(Body::empty())
        .unwrap();
    assert_eq!(sende(app_ohne_dist(), req).await.0, StatusCode::FORBIDDEN);
    // Ohne bekannte Gegenstelle wird abgewiesen, nicht durchgelassen.
    let req = Request::get("/api/workspace")
        .header("host", "127.0.0.1:8787")
        .body(Body::empty())
        .unwrap();
    assert_eq!(sende(app_ohne_dist(), req).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn host_wird_gegen_den_konfigurierten_port_geprueft() {
    let state = AppState::mit_port(
        db::open_im_speicher().unwrap(),
        Arc::new(MemoryStore::new()),
        9000,
    );
    let app = || router(state.clone(), PathBuf::from("/x"));
    assert_eq!(hole(app(), "/api/health").await.0, StatusCode::FORBIDDEN);
    let req = Request::get("/api/health")
        .header("host", "localhost:9000")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
        .body(Body::empty())
        .unwrap();
    assert_eq!(sende(app(), req).await.0, StatusCode::OK);
}

// ---------- GET /api/events ----------

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

#[tokio::test]
async fn ereignisse_melden_revision_und_neue_eintraege() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));

    let antwort = app()
        .oneshot(anfrage("GET", "/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(antwort.status(), StatusCode::OK);
    assert_eq!(
        antwort.headers()[CONTENT_TYPE].to_str().unwrap(),
        "text/event-stream"
    );

    let (status, _) = schicke(app(), &mark_done(&erste_id())).await;
    assert_eq!(status, StatusCode::OK);

    let ereignisse = lese_ereignisse(antwort.into_body(), 2).await;
    // Erst der Stand beim Verbinden, dann die Aktion.
    assert_eq!(ereignisse[0]["revision"], 0);
    assert_eq!(ereignisse[0]["eintraege"], serde_json::json!([]));
    assert_eq!(ereignisse[1]["revision"], 1);
    let eintraege = ereignisse[1]["eintraege"].as_array().unwrap();
    assert_eq!(eintraege.len(), 1);
    assert_eq!(eintraege[0]["actor"], "Du");
}

#[tokio::test]
async fn fehlgeschlagene_aktion_sendet_kein_ereignis() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let antwort = app()
        .oneshot(anfrage("GET", "/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (status, _) = schicke(app(), r#"{"action":{"MarkDone":{"item":"nein"}}}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = schicke(app(), &mark_done(&erste_id())).await;
    assert_eq!(status, StatusCode::OK);
    let ereignisse = lese_ereignisse(antwort.into_body(), 2).await;
    assert_eq!(ereignisse[1]["revision"], 1, "{ereignisse:?}");
}

// ---------- Agent-Takt ----------

#[tokio::test]
async fn takt_laesst_den_agenten_arbeiten_als_agent() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    // Ohne laufenden Agenten ändert der Takt nichts.
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    assert_eq!(state.inner.live.revision(), 0);

    let ws = cockpit_core::demo::workspace();
    let ticket = ws
        .items
        .iter()
        .find(|i| i.key.starts_with("BAU-"))
        .expect("Ticket in den Beispieldaten")
        .key
        .clone();
    let start = serde_json::to_string(&ActionRequest {
        action: cockpit_core::Action::StartAgent {
            ticket: ticket.clone(),
            options: cockpit_core::AgentOptions {
                plan_first: false,
                ..Default::default()
            },
        },
    })
    .unwrap();
    let (status, body) = schicke(app(), &start).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let vorher = state.inner.live.revision();

    let s = state.clone();
    tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    assert_eq!(state.inner.live.revision(), vorher + 1);
    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(ws.log.last().unwrap().actor, Actor::Agent);
}

#[tokio::test]
async fn takt_verknuepft_pull_requests_und_fuehrt_den_status_nach() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    // Der Branch von #85 nennt BAU-150: Der Takt verknüpft ihn (als System).
    let mut ws = cockpit_core::demo::workspace();
    ws.reviews
        .iter_mut()
        .find(|r| r.number == 85)
        .unwrap()
        .branch = "feature/bau-150-sheet".into();
    state.inner.live.setze_stand_fuer_test(ws);
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(
        ws.review(85).unwrap().ticket_key.as_deref(),
        Some("BAU-150")
    );
    assert!(ws.log.iter().all(|e| e.actor == Actor::System));
    assert!(ws.log.len() >= 2, "Verknüpfung und Status: {:?}", ws.log);
    // Beim zweiten Takt gibt es nichts mehr zu tun.
    let vorher = state.inner.live.revision();
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    assert_eq!(state.inner.live.revision(), vorher);
}

#[tokio::test]
async fn takt_treibt_folgeauftraege_bis_zur_diagnose() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let start = serde_json::to_string(&ActionRequest {
        action: cockpit_core::Action::InvestigateCheck {
            review: 85,
            check: "Lint".into(),
        },
    })
    .unwrap();
    let (status, body) = schicke(app(), &start).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for _ in 0..3 {
        let s = state.clone();
        tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    }
    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
    let lauf = ws.run("check-85-lint").unwrap();
    assert!(lauf.is_diagnosed(), "wartet auf deine Entscheidung");
    assert!(lauf.diagnosis.is_some());
    // Danach ruht der Takt, bis du den Fix übernimmst.
    let vorher = state.inner.live.revision();
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    assert_eq!(state.inner.live.revision(), vorher);
    // Auch jetzt lässt sich nichts mergen: #85 hat rote Checks.
    let merge = serde_json::to_string(&ActionRequest {
        action: cockpit_core::Action::MergePr {
            review: 85,
            method: cockpit_core::MergeMethod::Squash,
        },
    })
    .unwrap();
    let (status, _) = schicke(app(), &merge).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn takt_holt_zurueckgestellte_eintraege_zurueck() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let id = erste_id();
    let bis = jetzt_ms() + 60_000;
    let snooze = serde_json::to_string(&ActionRequest {
        action: cockpit_core::Action::Snooze {
            item: id.clone(),
            until_ms: Some(bis),
        },
    })
    .unwrap();
    let (status, body) = schicke(app(), &snooze).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let vorher = state.inner.live.revision();

    // Vor Ablauf tut der Takt nichts …
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.inner.live.takt(&s.inner.db, bis - 1, true))
        .await
        .unwrap();
    assert_eq!(state.inner.live.revision(), vorher);
    // … danach ist der Eintrag zurück, protokolliert als System.
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.inner.live.takt(&s.inner.db, bis + 1, true))
        .await
        .unwrap();
    assert_eq!(state.inner.live.revision(), vorher + 1);
    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert!(ws.snoozed.is_empty());
    assert_eq!(ws.log.last().unwrap().actor, Actor::System);
}

// ---------- Persistenz ----------

#[tokio::test]
async fn stand_ueberlebt_den_neustart() {
    let tmp = tempfile::tempdir().unwrap();
    let datei = tmp.path().join("cockpit.db");
    let id = erste_id();

    let nach_aktion = {
        let state = AppState::new(db::open(&datei).unwrap(), Arc::new(MemoryStore::new()));
        let app = router(state.clone(), PathBuf::from("/x"));
        let (status, _) = schicke(app.clone(), &mark_done(&id)).await;
        assert_eq!(status, StatusCode::OK);
        hole(app, "/api/workspace").await.1
    };

    // Neuer Prozess: dieselbe Datei, frischer Zustand.
    let state = AppState::new(db::open(&datei).unwrap(), Arc::new(MemoryStore::new()));
    let app = router(state, PathBuf::from("/x"));
    let (_, nach_neustart) = hole(app.clone(), "/api/workspace").await;
    assert_eq!(nach_neustart, nach_aktion);
    let (ws, revision) = cockpit_core::persist::load_with_revision(&nach_neustart).unwrap();
    assert_eq!(revision, 1);
    assert!(ws.item(&id).unwrap().read);
    // Und es geht dort weiter, wo es aufgehört hat.
    let (_, body) = schicke(
        app,
        &format!(r#"{{"action":{{"SetStatus":{{"item":"{id}","status":"Review"}}}}}}"#),
    )
    .await;
    assert_eq!(json(&body)["revision"], 2, "{body}");
}

#[tokio::test]
async fn unlesbarer_gespeicherter_stand_faellt_auf_demo_zurueck() {
    let conn = db::open_im_speicher().unwrap();
    db::workspace_speichern(&conn, 7, "{kaputt").unwrap();
    let state = AppState::new(conn, Arc::new(MemoryStore::new()));
    let app = router(state, PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/workspace").await;
    let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(ws, cockpit_core::demo::workspace());
    assert_eq!(revision, 0);
}

// ---------- Seed, LiteLLM, Container ----------

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

#[tokio::test]
async fn ohne_seed_startet_der_workspace_leer() {
    let conn = db::open_im_speicher().unwrap();
    let (ws, revision) = stand_von(zustand_mit(conn, leer(), Arc::new(MemoryStore::new()))).await;
    assert_eq!(ws, cockpit_core::Workspace::default());
    assert!(ws.items.is_empty() && ws.reviews.is_empty() && ws.runs.is_empty());
    assert_eq!(revision, 0);
}

#[tokio::test]
async fn seed_demo_laedt_die_beispieldaten() {
    let e = Einstellungen {
        seed: Seed::Demo,
        ..leer()
    };
    let conn = db::open_im_speicher().unwrap();
    let (ws, _) = stand_von(zustand_mit(conn, e, Arc::new(MemoryStore::new()))).await;
    assert_eq!(ws, cockpit_core::demo::workspace());
}

#[tokio::test]
async fn ein_gespeicherter_stand_geht_dem_seed_vor() {
    let mut gespeichert = cockpit_core::demo::workspace();
    let id = gespeichert.items[0].id.clone();
    gespeichert.mark_done(&id);
    let text = cockpit_core::persist::save_with_revision(&gespeichert, 5);
    for seed in [Seed::Leer, Seed::Demo] {
        let conn = db::open_im_speicher().unwrap();
        db::workspace_speichern(&conn, 5, &text).unwrap();
        let e = Einstellungen { seed, ..leer() };
        let (ws, revision) = stand_von(zustand_mit(conn, e, Arc::new(MemoryStore::new()))).await;
        assert_eq!(ws, gespeichert, "{seed:?}");
        assert_eq!(revision, 5);
    }
}

#[tokio::test]
async fn unlesbarer_stand_faellt_auf_den_seed_zurueck() {
    let conn = db::open_im_speicher().unwrap();
    db::workspace_speichern(&conn, 7, "{kaputt").unwrap();
    let (ws, revision) = stand_von(zustand_mit(conn, leer(), Arc::new(MemoryStore::new()))).await;
    assert_eq!(ws, cockpit_core::Workspace::default());
    assert_eq!(revision, 0);
}

#[tokio::test]
async fn aktionen_auf_dem_leeren_workspace_lehnen_unbekannte_eintraege_ab() {
    let conn = db::open_im_speicher().unwrap();
    let app = router(
        zustand_mit(conn, leer(), Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (status, body) = schicke(app, &mark_done("gibt-es-nicht")).await;
    assert!(status.is_client_error(), "{status} {body}");
}

#[tokio::test]
async fn health_meldet_den_endpunkt_ohne_den_key() {
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-geheim-123456"))
        .unwrap();
    let e = Einstellungen {
        agent: AgentWahl {
            anbieter: Provider::ClaudeAgentSdk,
            endpunkt: Some(EndpunktKonfig {
                url: Some("http://localhost:4000".into()),
                modell: Some("claude-sonnet-5-5".into()),
            }),
        },
        ..leer()
    };
    let conn = db::open_im_speicher().unwrap();
    let app = router(zustand_mit(conn, e, secrets), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    assert!(!body.contains("geheim"), "{body}");
    let j = json(&body);
    assert_eq!(j["endpunkt"]["konfiguriert"], true);
    assert_eq!(j["endpunkt"]["anbieter"], "claude-agent-sdk");
    assert_eq!(j["endpunkt"]["url"], "http://localhost:4000");
    assert_eq!(j["endpunkt"]["modell"], "claude-sonnet-5-5");
    assert_eq!(j["endpunkt"]["key"], true);

    // Der ältere Schlüsselname `litellm` zählt weiter.
    let alt = Arc::new(MemoryStore::new());
    alt.set(secrets::LITELLM, &Secret::new("sk-alt-123456"))
        .unwrap();
    let app = router(zustand(alt), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    assert_eq!(json(&body)["endpunkt"]["key"], true);

    // Ohne Einstellung: nicht konfiguriert, kein Key.
    let app = router(zustand(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["endpunkt"]["konfiguriert"], false);
    assert_eq!(j["endpunkt"]["key"], false);
    assert_eq!(j["container"]["quelle"], "keine");
    assert_eq!(j["container"]["laufzeit"], serde_json::Value::Null);
}

#[cfg(unix)]
#[tokio::test]
async fn health_meldet_die_container_laufzeit() {
    use std::os::unix::fs::PermissionsExt;
    let ordner = tempfile::tempdir().unwrap();
    let datei = ordner.path().join("podman");
    std::fs::write(&datei, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&datei, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = Einstellungen {
        suchpfad: Some(ordner.path().as_os_str().to_owned()),
        ..Einstellungen::default()
    };
    let conn = db::open_im_speicher().unwrap();
    let app = router(
        zustand_mit(conn, e, Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["container"]["laufzeit"], "podman");
    assert_eq!(j["container"]["engine_laeuft"], true);
    assert_eq!(j["container"]["quelle"], "erkannt");
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

#[tokio::test]
async fn pruefen_meldet_erreichbar_key_und_modell_unter_beiden_adressen() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-geheim-123456"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [{"id": "claude-sonnet-5-5"}]})),
        )
        .expect(2)
        .mount(&proxy)
        .await;
    // Der Key liegt unter dem älteren Namen `litellm`.
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::LITELLM, &Secret::new("sk-geheim-123456"))
        .unwrap();
    for pfad in ["/api/agent/pruefen", "/api/litellm/pruefen"] {
        let (status, body) = hole(
            mit_endpunkt(Some(&proxy.uri()), None, secrets.clone()),
            pfad,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("geheim"), "{body}");
        let j = json(&body);
        assert_eq!(j["konfiguriert"], true, "{pfad}");
        assert_eq!(j["erreichbar"], true);
        assert_eq!(j["key_ok"], true);
        assert_eq!(j["modell_vorhanden"], true);
    }
}

#[tokio::test]
async fn pruefen_ohne_key_ruft_den_endpunkt_nicht_auf() {
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&proxy)
        .await;
    let (_, body) = hole(
        mit_endpunkt(Some(&proxy.uri()), None, Arc::new(MemoryStore::new())),
        "/api/agent/pruefen",
    )
    .await;
    let j = json(&body);
    assert_eq!(j["konfiguriert"], true);
    assert_eq!(j["key_ok"], false);
    assert!(j["hinweis"].as_str().unwrap().contains("agent-key"));
}

#[tokio::test]
async fn pruefen_ohne_einstellung_und_von_fremden_hosts() {
    let app = router(zustand(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
    for pfad in ["/api/agent/pruefen", "/api/litellm/pruefen"] {
        let (status, body) = hole(app.clone(), pfad).await;
        assert_eq!(status, StatusCode::OK);
        let j = json(&body);
        assert_eq!(j["konfiguriert"], false);
        assert!(j["hinweis"].as_str().unwrap().contains("Modell"));
    }
    for pfad in ["/api/agent/modelle", "/api/litellm/pruefen"] {
        let req = Request::builder()
            .uri(pfad)
            .header("host", "boese.example")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::empty())
            .unwrap();
        assert_eq!(sende(app.clone(), req).await.0, StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn modellliste_vom_proxy_mit_bearer_und_ohne_den_key() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-geheim-123456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"data": [{"id": "gpt-x"}, {"id": "claude-b"}, {"id": "claude-a"}]}),
        ))
        .expect(1)
        .mount(&proxy)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-geheim-123456"))
        .unwrap();
    let (status, body) = hole(
        mit_endpunkt(Some(&proxy.uri()), None, secrets),
        "/api/agent/modelle",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("geheim"), "{body}");
    let j = json(&body);
    assert_eq!(j["geladen"], true);
    assert_eq!(
        j["modelle"],
        serde_json::json!(["claude-a", "claude-b", "gpt-x"])
    );
}

#[tokio::test]
async fn modellliste_direkt_nimmt_x_api_key() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let anthropic = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", "sk-ant-geheim"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [{"id": "claude-opus-5-5"}]})),
        )
        .expect(1)
        .mount(&anthropic)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-ant-geheim"))
        .unwrap();
    let (_, body) = hole(
        mit_endpunkt(None, Some(&anthropic.uri()), secrets),
        "/api/agent/modelle",
    )
    .await;
    assert!(!body.contains("geheim"), "{body}");
    assert_eq!(
        json(&body)["modelle"],
        serde_json::json!(["claude-opus-5-5"])
    );
}

#[tokio::test]
async fn modellliste_meldet_abgelehnten_key_und_fehlenden_key_in_klartext() {
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&proxy)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-falsch-1"))
        .unwrap();
    let (_, body) = hole(
        mit_endpunkt(Some(&proxy.uri()), None, secrets),
        "/api/agent/modelle",
    )
    .await;
    let j = json(&body);
    assert_eq!(j["geladen"], false);
    assert_eq!(j["erreichbar"], true);
    assert!(j["hinweis"].as_str().unwrap().contains("lehnt den Key ab"));
    assert!(!body.contains("falsch"), "{body}");

    // Ohne Key wird nichts aufgerufen.
    let still = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&still)
        .await;
    let (_, body) = hole(
        mit_endpunkt(Some(&still.uri()), None, Arc::new(MemoryStore::new())),
        "/api/agent/modelle",
    )
    .await;
    assert!(
        json(&body)["hinweis"]
            .as_str()
            .unwrap()
            .contains("kein Key")
    );
}

#[tokio::test]
async fn health_meldet_den_agenten_nie_geheimnisse() {
    // Simuliert (Tests starten mit dem Seed demo): immer bereit.
    let app = router(zustand(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["agent"]["modus"], "simuliert");
    assert_eq!(j["agent"]["bereit"], true);
    // Echt, aber nichts eingerichtet: sagt in Klartext, was fehlt.
    let conn = db::open_im_speicher().unwrap();
    let e = Einstellungen {
        seed: Seed::Leer,
        ..leer()
    };
    let app = router(
        zustand_mit(conn, e, Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["agent"]["modus"], "echt");
    assert_eq!(j["agent"]["bereit"], false);
    assert_eq!(j["agent"]["image_name"], "localhost/cockpit-agent:1");
    let fehlt = j["agent"]["fehlt"].as_array().unwrap();
    assert!(fehlt.len() >= 3);
    assert!(
        fehlt
            .iter()
            .all(|f| f.as_str().unwrap().contains("Einrichten"))
    );
}

#[tokio::test]
async fn image_bauen_ohne_laufzeit_ist_422_mit_klartext() {
    let conn = db::open_im_speicher().unwrap();
    let e = Einstellungen {
        seed: Seed::Leer,
        ..leer()
    };
    let app = router(
        zustand_mit(conn, e, Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (status, body) = hole(app.clone(), "/api/agent/image").await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&body);
    assert_eq!(j["laeuft"], false);
    assert_eq!(j["vorhanden"], false);
    let req = anfrage("POST", "/api/agent/image")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let (status, body) = sende(app, req).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json(&body)["fehler"]
            .as_str()
            .unwrap()
            .contains("Rancher Desktop"),
        "{body}"
    );
}
