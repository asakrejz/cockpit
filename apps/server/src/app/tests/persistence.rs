use super::*;

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
