use super::*;

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

#[tokio::test]
async fn health_nennt_die_env_datei_nie_ihren_inhalt() {
    let dir = tempfile::tempdir().unwrap();
    let datei = dir.path().join(".env");
    let store = Arc::new(secrets::EnvFileStore::with_env(datei.clone(), |_| None));
    store
        .set(secrets::GITHUB, &Secret::new("dummy-streng-geheim"))
        .unwrap();
    let state = AppState::new(db::open_im_speicher().unwrap(), store);
    let app = router(state, PathBuf::from("/gibt/es/nicht"));
    let (status, body) = hole(app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("dummy-streng-geheim"), "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["env_datei"]["path"], datei.display().to_string());
    assert!(json["env_datei"]["warnings"].is_array());
    // Mit dem Speicher für Tests gibt es keine Datei.
    let app = router(
        zustand(Arc::new(MemoryStore::new())),
        PathBuf::from("/gibt/es/nicht"),
    );
    let (_, body) = hole(app, "/api/health").await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["env_datei"].is_null());
}
