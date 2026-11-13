use super::*;

#[tokio::test]
async fn health_without_key() {
    let app = router(
        state(Arc::new(MemoryStore::new())),
        PathBuf::from("/gibt/es/nicht"),
    );
    let (status, body) = hole(app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["version"], VERSION);
    assert_eq!(json["connections"]["github"], false);
    assert_eq!(json["connections"]["jira"], false);
}

#[tokio::test]
async fn health_shows_only_ja_no_never_the_key() {
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::GITHUB, &Secret::new("ghp_streng_geheim"))
        .unwrap();
    let app = router(state(secrets), PathBuf::from("/gibt/es/nicht"));
    let (status, body) = hole(app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("ghp_streng_geheim"), "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["connections"]["github"], true);
    assert_eq!(json["connections"]["jira"], false);
}

#[tokio::test]
async fn key_without_connection_counts_not() {
    let secrets = Arc::new(MemoryStore::new());
    secrets.set(secrets::JIRA, &Secret::new("t")).unwrap();
    let state = AppState::new(db::open_in_memory().unwrap(), secrets);
    let h = state.health();
    assert_eq!(
        h.connections,
        Connections {
            github: false,
            jira: false
        }
    );
}

#[tokio::test]
async fn unknown_api_address_is_404_json() {
    let app = router(
        state(Arc::new(MemoryStore::new())),
        PathBuf::from("/gibt/es/nicht"),
    );
    let (status, body) = hole(app, "/api/gibt-es-nicht").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("error"));
}

#[tokio::test]
async fn returns_the_app_as_static_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("index.html"), "<h1>Cockpit</h1>").unwrap();
    std::fs::write(tmp.path().join("app.js"), "console.log(1)").unwrap();
    let app = || {
        router(
            state(Arc::new(MemoryStore::new())),
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
async fn health_names_the_env_file_never_its_content() {
    let dir = tempfile::tempdir().unwrap();
    let datei = dir.path().join(".env");
    let store = Arc::new(secrets::EnvFileStore::with_env(datei.clone(), |_| None));
    store
        .set(secrets::GITHUB, &Secret::new("dummy-streng-geheim"))
        .unwrap();
    let with_file = AppState::new(db::open_in_memory().unwrap(), store);
    let app = router(with_file, PathBuf::from("/gibt/es/nicht"));
    let (status, body) = hole(app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("dummy-streng-geheim"), "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["env_file"]["path"], datei.display().to_string());
    assert!(json["env_file"]["warnings"].is_array());
    // Mit dem Speicher für Tests gibt es keine Datei.
    let app = router(
        state(Arc::new(MemoryStore::new())),
        PathBuf::from("/gibt/es/nicht"),
    );
    let (_, body) = hole(app, "/api/health").await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["env_file"].is_null());
}
