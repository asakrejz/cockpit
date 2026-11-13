use super::*;

// ---------- Persistenz ----------

#[tokio::test]
async fn state_survives_the_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("cockpit.db");
    let id = first_id();

    let after_action = {
        let state = AppState::new(db::open(&file).unwrap(), Arc::new(MemoryStore::new()));
        let app = router(state.clone(), PathBuf::from("/x"));
        let (status, _) = send(app.clone(), &mark_done(&id)).await;
        assert_eq!(status, StatusCode::OK);
        hole(app, "/api/workspace").await.1
    };

    // New process: same file, fresh state.
    let state = AppState::new(db::open(&file).unwrap(), Arc::new(MemoryStore::new()));
    let app = router(state, PathBuf::from("/x"));
    let (_, after_restart) = hole(app.clone(), "/api/workspace").await;
    assert_eq!(after_restart, after_action);
    let (ws, revision) = cockpit_core::persist::load_with_revision(&after_restart).unwrap();
    assert_eq!(revision, 1);
    assert!(ws.item(&id).unwrap().read);
    // And it continues where it stopped.
    let (_, body) = send(
        app,
        &format!(r#"{{"action":{{"SetStatus":{{"item":"{id}","status":"Review"}}}}}}"#),
    )
    .await;
    assert_eq!(json(&body)["revision"], 2, "{body}");
}

#[tokio::test]
async fn unreadable_stored_state_falls_on_demo_back() {
    let conn = db::open_in_memory().unwrap();
    db::workspace_save(&conn, 7, "{kaputt").unwrap();
    let state = AppState::new(conn, Arc::new(MemoryStore::new()));
    let app = router(state, PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/workspace").await;
    let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(ws, cockpit_core::demo::workspace());
    assert_eq!(revision, 0);
}
