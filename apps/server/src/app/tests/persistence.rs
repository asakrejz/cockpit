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
    assert_eq!(ws, cockpit_core::demo::multi_repo_workspace());
    assert_eq!(revision, 0);
}

/// A database written before several repos were supported (no `repo` on reviews, pull request
/// references and runs) opens with the same data; the repos are filled in and stored.
#[tokio::test]
async fn an_old_database_without_repos_opens_with_the_same_data() {
    let old = include_str!(
        "../../../../../crates/cockpit-core/tests/fixtures/wire_pre_repo_busy_workspace.json"
    );
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("cockpit.db");
    {
        let conn = db::open(&file).unwrap();
        db::workspace_save(&conn, 7, old).unwrap();
    }
    let (before, _) = cockpit_core::persist::load_with_revision(old).unwrap();
    assert!(before.reviews.iter().all(|r| r.repo.is_empty()));

    let settings = Settings {
        agent_repo: Some("firma/app".into()),
        ..Settings::default()
    };
    let state = AppState::create(
        db::open(&file).unwrap(),
        Arc::new(MemoryStore::new()),
        8787,
        crate::sync::Sources::default(),
        settings,
    );
    let ws = state.inner.live.copy();
    // Same data …
    assert_eq!(
        ws.items,
        before
            .items
            .iter()
            .cloned()
            .map(|mut i| {
                if let Some(p) = i.pr.as_mut().filter(|p| p.repo.is_empty()) {
                    p.repo = ws
                        .item(&i.id)
                        .and_then(|n| n.pr.as_ref().map(|n| n.repo.clone()))
                        .unwrap_or_default();
                }
                i
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(ws.reviews.len(), before.reviews.len());
    assert_eq!(ws.log, before.log);
    // … with every review in a repo, and the entries' own repo kept.
    assert!(ws.reviews.iter().all(|r| !r.repo.is_empty()));
    assert_eq!(
        ws.item("pr12").unwrap().pr.as_ref().unwrap().repo,
        "asakrejz/design-tokens"
    );
    // It is stored: the next start reads it without a change.
    assert_eq!(state.inner.live.revision(), 8);
    let stored = db::load_workspace(&db::open(&file).unwrap())
        .unwrap()
        .unwrap();
    let (back, _) = cockpit_core::persist::load_with_revision(&stored).unwrap();
    assert_eq!(back, ws);
}
