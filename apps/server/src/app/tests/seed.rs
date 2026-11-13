use super::*;

#[tokio::test]
async fn without_seed_starts_the_workspace_empty() {
    let conn = db::open_in_memory().unwrap();
    let (ws, revision) = state_from(state_with(conn, empty(), Arc::new(MemoryStore::new()))).await;
    assert_eq!(ws, cockpit_core::Workspace::default());
    assert!(ws.items.is_empty() && ws.reviews.is_empty() && ws.runs.is_empty());
    assert_eq!(revision, 0);
}

#[tokio::test]
async fn seed_demo_loads_the_sample_data() {
    let e = Settings {
        seed: Seed::Demo,
        ..empty()
    };
    let conn = db::open_in_memory().unwrap();
    let (ws, _) = state_from(state_with(conn, e, Arc::new(MemoryStore::new()))).await;
    assert_eq!(ws, cockpit_core::demo::multi_repo_workspace());
}

#[tokio::test]
async fn a_stored_state_goes_the_seed_before() {
    let mut stored = cockpit_core::demo::workspace();
    let id = stored.items[0].id.clone();
    stored.mark_done(&id);
    let text = cockpit_core::persist::save_with_revision(&stored, 5);
    for seed in [Seed::Empty, Seed::Demo] {
        let conn = db::open_in_memory().unwrap();
        db::workspace_save(&conn, 5, &text).unwrap();
        let e = Settings { seed, ..empty() };
        let (ws, revision) = state_from(state_with(conn, e, Arc::new(MemoryStore::new()))).await;
        assert_eq!(ws, stored, "{seed:?}");
        assert_eq!(revision, 5);
    }
}

#[tokio::test]
async fn unreadable_state_falls_on_the_seed_back() {
    let conn = db::open_in_memory().unwrap();
    db::workspace_save(&conn, 7, "{kaputt").unwrap();
    let (ws, revision) = state_from(state_with(conn, empty(), Arc::new(MemoryStore::new()))).await;
    assert_eq!(ws, cockpit_core::Workspace::default());
    assert_eq!(revision, 0);
}

#[tokio::test]
async fn actions_on_the_empty_workspace_reject_unknown_entries_ab() {
    let conn = db::open_in_memory().unwrap();
    let app = router(
        state_with(conn, empty(), Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (status, body) = send(app, &mark_done("gibt-es-nicht")).await;
    assert!(status.is_client_error(), "{status} {body}");
}

#[tokio::test]
async fn health_reports_the_endpoint_without_the_key() {
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-geheim-123456"))
        .unwrap();
    let e = Settings {
        agent: AgentChoice {
            provider: Provider::ClaudeAgentSdk,
            endpoint: Some(EndpointConfig {
                url: Some("http://localhost:4000".into()),
                model: Some("claude-sonnet-5-5".into()),
            }),
        },
        ..empty()
    };
    let conn = db::open_in_memory().unwrap();
    let app = router(state_with(conn, e, secrets), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    assert!(!body.contains("geheim"), "{body}");
    let j = json(&body);
    assert_eq!(j["endpoint"]["configured"], true);
    assert_eq!(j["endpoint"]["provider"], "claude-agent-sdk");
    assert_eq!(j["endpoint"]["url"], "http://localhost:4000");
    assert_eq!(j["endpoint"]["model"], "claude-sonnet-5-5");
    assert_eq!(j["endpoint"]["key"], true);

    // The older key name `litellm` still counts.
    let alt = Arc::new(MemoryStore::new());
    alt.set(secrets::LITELLM, &Secret::new("sk-alt-123456"))
        .unwrap();
    let app = router(state(alt), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    assert_eq!(json(&body)["endpoint"]["key"], true);

    // Without a setting: not configured, no key.
    let app = router(state(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["endpoint"]["configured"], false);
    assert_eq!(j["endpoint"]["key"], false);
    assert_eq!(j["container"]["source"], "none");
    assert_eq!(j["container"]["runtime"], serde_json::Value::Null);
}

#[cfg(unix)]
#[tokio::test]
async fn health_reports_the_container_runtime() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("podman");
    std::fs::write(&file, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = Settings {
        search_path: Some(dir.path().as_os_str().to_owned()),
        ..Settings::default()
    };
    let conn = db::open_in_memory().unwrap();
    let app = router(
        state_with(conn, e, Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["container"]["runtime"], "podman");
    assert_eq!(j["container"]["engine_running"], true);
    assert_eq!(j["container"]["source"], "detected");
}
