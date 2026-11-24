use super::*;

// ---------- Tick ----------

#[tokio::test]
async fn tick_without_anything_to_do_changes_nothing() {
    let state = state(Arc::new(MemoryStore::new()));
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.tick()).await.unwrap();
    assert_eq!(state.inner.live.revision(), 0);
}

#[tokio::test]
async fn tick_linked_pull_requests_and_leads_the_status_after() {
    let state = state(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    // The branch of #85 names BAU-150: the tick links it (as system).
    let mut ws = cockpit_core::demo::workspace();
    ws.reviews
        .iter_mut()
        .find(|r| r.number == 85)
        .unwrap()
        .branch = "feature/bau-150-sheet".into();
    state.inner.live.set_state_for_test(ws);
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.tick()).await.unwrap();
    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(
        ws.review(85).unwrap().ticket_key.as_deref(),
        Some("BAU-150")
    );
    assert!(ws.log.iter().all(|e| e.actor == Actor::System));
    assert!(ws.log.len() >= 2, "link and status: {:?}", ws.log);
    // On the second tick there is nothing left to do.
    let before = state.inner.live.revision();
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.tick()).await.unwrap();
    assert_eq!(state.inner.live.revision(), before);
}

#[tokio::test]
async fn tick_brings_snoozed_entries_back() {
    let state = state(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let id = first_id();
    let bis = now_ms() + 60_000;
    let snooze = serde_json::to_string(&ActionRequest {
        action: cockpit_core::Action::Snooze {
            item: id.clone(),
            until_ms: Some(bis),
        },
    })
    .unwrap();
    let (status, body) = send(app(), &snooze).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let before = state.inner.live.revision();

    // Before it expires the tick does nothing …
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.inner.live.tick(&s.inner.db, bis - 1))
        .await
        .unwrap();
    assert_eq!(state.inner.live.revision(), before);
    // … afterwards the entry is back, logged as system.
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.inner.live.tick(&s.inner.db, bis + 1))
        .await
        .unwrap();
    assert_eq!(state.inner.live.revision(), before + 1);
    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert!(ws.snoozed.is_empty());
    assert_eq!(ws.log.last().unwrap().actor, Actor::System);
}
