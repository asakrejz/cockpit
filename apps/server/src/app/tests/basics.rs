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
    let file = dir.path().join(".env");
    let store = Arc::new(secrets::EnvFileStore::with_env(file.clone(), |_| None));
    store
        .set(secrets::GITHUB, &Secret::new("dummy-streng-geheim"))
        .unwrap();
    let with_file = AppState::new(db::open_in_memory().unwrap(), store);
    let app = router(with_file, PathBuf::from("/gibt/es/nicht"));
    let (status, body) = hole(app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("dummy-streng-geheim"), "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["env_file"]["path"], file.display().to_string());
    assert!(json["env_file"]["warnings"].is_array());
    // The in-memory store used for tests has no file.
    let app = router(
        state(Arc::new(MemoryStore::new())),
        PathBuf::from("/gibt/es/nicht"),
    );
    let (_, body) = hole(app, "/api/health").await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["env_file"].is_null());
}

#[tokio::test]
async fn health_reports_the_config_folder_and_old_files_until_they_are_gone() {
    let project = tempfile::tempdir().unwrap();
    let old = tempfile::tempdir().unwrap();
    let old_file = old.path().join("config.toml");
    std::fs::write(&old_file, "port = 1\n").unwrap();
    let config = project.path().join("config");
    std::fs::create_dir(&config).unwrap();
    let settings = Settings {
        config_dir: Some(config.clone()),
        legacy_files: vec![old_file.clone()],
        ..Default::default()
    };
    let json = |body: String| serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let get = || {
        let s = state_with(
            db::open_in_memory().unwrap(),
            settings.clone(),
            Arc::new(MemoryStore::new()),
        );
        hole(router(s, PathBuf::from("/gibt/es/nicht")), "/api/health")
    };
    let (status, body) = get().await;
    assert_eq!(status, StatusCode::OK);
    let j = json(body);
    assert_eq!(j["config_files"]["path"], config.display().to_string());
    assert!(j["config_files"]["warnings"].is_array());
    let notices = j["config_files"]["notices"].as_array().unwrap();
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0]
            .as_str()
            .unwrap()
            .contains("Alte Konfiguration liegt noch unter")
    );
    // Reported only; the old file is untouched.
    assert!(old_file.is_file());
    std::fs::remove_file(&old_file).unwrap();
    let (_, body) = get().await;
    assert!(
        json(body)["config_files"]["notices"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Without a folder (both overrides set) the field is null.
    let s = state(Arc::new(MemoryStore::new()));
    let (_, body) = hole(router(s, PathBuf::from("/x")), "/api/health").await;
    assert!(json(body)["config_files"].is_null());
}
