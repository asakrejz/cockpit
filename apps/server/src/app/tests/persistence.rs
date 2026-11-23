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
    assert_eq!(ws, cockpit_core::demo::seed());
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
        "acme/design-tokens"
    );
    // It is stored: the next start reads it without a change.
    assert_eq!(state.inner.live.revision(), 8);
    let stored = db::load_workspace(&db::open(&file).unwrap())
        .unwrap()
        .unwrap();
    let (back, _) = cockpit_core::persist::load_with_revision(&stored).unwrap();
    assert_eq!(back, ws);
}

// ---------- Unreadable stored state ----------

/// Every row kept aside: revision, raw bytes of the stored value and the reason.
fn kept_rows(state: &AppState) -> Vec<(i64, Vec<u8>, String)> {
    let conn = state.inner.db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT revision, CAST(json AS BLOB), error FROM workspace_unreadable ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

async fn health_of(app: &Router) -> serde_json::Value {
    let (status, body) = hole(app.clone(), "/api/health").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    json(&body)
}

fn stored_text(state: &AppState) -> Option<String> {
    db::load_workspace(&state.inner.db.lock().unwrap()).unwrap()
}

#[tokio::test]
async fn a_corrupt_stored_state_is_kept_aside_unchanged() {
    let conn = db::open_in_memory().unwrap();
    db::workspace_save(&conn, 7, "{kaputt").unwrap();
    let state = AppState::new(conn, Arc::new(MemoryStore::new()));

    // Kept byte for byte, with its revision and the reason; no longer the stored workspace.
    let kept = kept_rows(&state);
    assert_eq!(kept.len(), 1);
    assert_eq!((kept[0].0, kept[0].1.as_slice()), (7, "{kaputt".as_bytes()));
    assert!(kept[0].2.contains("not valid JSON"), "{}", kept[0].2);
    assert_eq!(stored_text(&state), None);
    // The server starts from the seed.
    assert_eq!(state.inner.live.copy(), cockpit_core::demo::seed());
    assert_eq!(state.inner.live.revision(), 0);

    // Health says what happened, in a catalog message with a German plain text beside it.
    let app = router(state.clone(), PathBuf::from("/x"));
    let notice = &health_of(&app).await["unreadable_state"];
    assert_eq!(notice["count"], 1);
    assert_eq!(notice["kept_at"].as_str().unwrap().len(), 19);
    let msg: cockpit_i18n::Msg = serde_json::from_value(notice["notice"].clone()).unwrap();
    assert_eq!(msg.code, cockpit_i18n::Key::ServerStateUnreadable.id());
    for lang in cockpit_i18n::Lang::ALL {
        assert!(
            msg.render(lang).contains("workspace_unreadable"),
            "{lang:?}: {}",
            msg.render(lang)
        );
    }

    // The new state is saved by the next action, and the kept row stays as it was.
    let id = first_id();
    let (status, _) = send(app, &mark_done(&id)).await;
    assert_eq!(status, StatusCode::OK);
    let (ws, revision) =
        cockpit_core::persist::load_with_revision(&stored_text(&state).unwrap()).unwrap();
    assert_eq!(revision, 1);
    assert!(ws.item(&id).unwrap().read);
    assert_eq!(kept_rows(&state), kept);
}

#[tokio::test]
async fn a_state_that_does_not_fit_is_kept_aside_unchanged() {
    // A renamed or retyped required field: valid JSON, right version, wrong shape.
    let mut wrong_shape: serde_json::Value = serde_json::from_str(
        &cockpit_core::persist::save_with_revision(&cockpit_core::demo::seed(), 4),
    )
    .unwrap();
    wrong_shape["workspace"]["items"] = serde_json::json!("not a list");
    let cases = [
        (
            r#"{"version":999,"workspace":{}}"#.to_string(),
            "unknown version 999",
        ),
        (r#"{"workspace":{}}"#.to_string(), "no valid version number"),
        (wrong_shape.to_string(), "does not fit the workspace"),
        (String::new(), "not valid JSON"),
    ];
    for (raw, reason) in cases {
        let conn = db::open_in_memory().unwrap();
        db::workspace_save(&conn, 3, &raw).unwrap();
        let state = AppState::new(conn, Arc::new(MemoryStore::new()));

        let kept = kept_rows(&state);
        assert_eq!(kept.len(), 1, "{reason}");
        assert_eq!((kept[0].0, kept[0].1.as_slice()), (3, raw.as_bytes()));
        assert!(kept[0].2.contains(reason), "{reason}: {}", kept[0].2);
        assert_eq!(stored_text(&state), None, "{reason}");
        assert_eq!(state.inner.live.copy(), cockpit_core::demo::seed());

        let app = router(state, PathBuf::from("/x"));
        assert_eq!(health_of(&app).await["unreadable_state"]["count"], 1);
    }
}

#[tokio::test]
async fn a_stored_value_that_is_not_text_is_kept_aside_too() {
    let conn = db::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO workspace (id, revision, json) VALUES (1, 3, x'FF00FE')",
        [],
    )
    .unwrap();
    let state = AppState::new(conn, Arc::new(MemoryStore::new()));
    let kept = kept_rows(&state);
    assert_eq!(kept.len(), 1);
    assert_eq!(
        (kept[0].0, kept[0].1.as_slice()),
        (3, &[0xFF, 0x00, 0xFE][..])
    );
    assert!(kept[0].2.contains("cannot be read"), "{}", kept[0].2);
    assert_eq!(state.inner.live.copy(), cockpit_core::demo::seed());
}

#[tokio::test]
async fn a_readable_stored_state_loads_as_before_and_nothing_is_kept() {
    let mut stored = cockpit_core::demo::workspace();
    let id = stored.items[0].id.clone();
    stored.mark_done(&id);
    let conn = db::open_in_memory().unwrap();
    db::workspace_save(
        &conn,
        5,
        &cockpit_core::persist::save_with_revision(&stored, 5),
    )
    .unwrap();
    let state = AppState::new(conn, Arc::new(MemoryStore::new()));

    assert_eq!(state.inner.live.copy(), stored);
    assert_eq!(state.inner.live.revision(), 5);
    assert!(kept_rows(&state).is_empty());
    let app = router(state, PathBuf::from("/x"));
    assert!(health_of(&app).await["unreadable_state"].is_null());
}

#[tokio::test]
async fn kept_states_survive_restarts_and_are_not_kept_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("cockpit.db");
    db::workspace_save(&db::open(&file).unwrap(), 7, "{kaputt").unwrap();

    // Start, stop, start again without any action in between: still one kept row.
    for _ in 0..2 {
        let state = AppState::new(db::open(&file).unwrap(), Arc::new(MemoryStore::new()));
        assert_eq!(kept_rows(&state).len(), 1);
        let app = router(state, PathBuf::from("/x"));
        assert_eq!(health_of(&app).await["unreadable_state"]["count"], 1);
    }

    // After an action the saved state is read again; the kept row is still there.
    let state = AppState::new(db::open(&file).unwrap(), Arc::new(MemoryStore::new()));
    let app = router(state, PathBuf::from("/x"));
    let (status, _) = send(app, &mark_done(&first_id())).await;
    assert_eq!(status, StatusCode::OK);
    let state = AppState::new(db::open(&file).unwrap(), Arc::new(MemoryStore::new()));
    assert_eq!(state.inner.live.revision(), 1);
    assert_eq!(kept_rows(&state).len(), 1);
    assert_eq!(kept_rows(&state)[0].1, b"{kaputt");
}

#[tokio::test]
async fn nothing_is_saved_when_the_unreadable_state_cannot_be_kept() {
    let conn = db::open_in_memory().unwrap();
    db::workspace_save(&conn, 7, "{kaputt").unwrap();
    // Keeping it aside fails (no table), so the server must not write over the row.
    conn.execute_batch("DROP TABLE workspace_unreadable")
        .unwrap();
    let state = AppState::new(conn, Arc::new(MemoryStore::new()));
    let app = router(state.clone(), PathBuf::from("/x"));

    let (status, _) = send(app.clone(), &mark_done(&first_id())).await;
    assert!(!status.is_success(), "{status}");
    assert_eq!(stored_text(&state).as_deref(), Some("{kaputt"));
    // Health still answers.
    assert!(health_of(&app).await["unreadable_state"].is_null());
}
