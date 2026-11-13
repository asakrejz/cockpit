use super::*;

// ---------- Agent-Takt ----------

#[tokio::test]
async fn takt_lets_the_agents_work_as_agent() {
    let state = state(Arc::new(MemoryStore::new()));
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
    let (status, body) = send(app(), &start).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let before = state.inner.live.revision();

    let s = state.clone();
    tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    assert_eq!(state.inner.live.revision(), before + 1);
    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(ws.log.last().unwrap().actor, Actor::Agent);
}

#[tokio::test]
async fn takt_linked_pull_requests_and_leads_the_status_after() {
    let state = state(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    // Der Branch von #85 nennt BAU-150: Der Takt verknüpft ihn (als System).
    let mut ws = cockpit_core::demo::workspace();
    ws.reviews
        .iter_mut()
        .find(|r| r.number == 85)
        .unwrap()
        .branch = "feature/bau-150-sheet".into();
    state.inner.live.set_state_for_test(ws);
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
    let before = state.inner.live.revision();
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    assert_eq!(state.inner.live.revision(), before);
}

#[tokio::test]
async fn takt_drives_follow_up_jobs_until_to_diagnose() {
    let state = state(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let start = serde_json::to_string(&ActionRequest {
        action: cockpit_core::Action::InvestigateCheck {
            review: 85,
            check: "Lint".into(),
        },
    })
    .unwrap();
    let (status, body) = send(app(), &start).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for _ in 0..3 {
        let s = state.clone();
        tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    }
    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
    let run = ws.run("check-85-lint").unwrap();
    assert!(run.is_diagnosed(), "wartet auf deine Entscheidung");
    assert!(run.diagnosis.is_some());
    // Danach ruht der Takt, bis du den Fix übernimmst.
    let before = state.inner.live.revision();
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.takt()).await.unwrap();
    assert_eq!(state.inner.live.revision(), before);
    // Auch jetzt lässt sich nichts mergen: #85 hat rote Checks.
    let merge = serde_json::to_string(&ActionRequest {
        action: cockpit_core::Action::MergePr {
            review: 85,
            method: cockpit_core::MergeMethod::Squash,
        },
    })
    .unwrap();
    let (status, _) = send(app(), &merge).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
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

    // Vor Ablauf tut der Takt nichts …
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.inner.live.takt(&s.inner.db, bis - 1, true))
        .await
        .unwrap();
    assert_eq!(state.inner.live.revision(), before);
    // … danach ist der Eintrag zurück, protokolliert als System.
    let s = state.clone();
    tokio::task::spawn_blocking(move || s.inner.live.takt(&s.inner.db, bis + 1, true))
        .await
        .unwrap();
    assert_eq!(state.inner.live.revision(), before + 1);
    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, _) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert!(ws.snoozed.is_empty());
    assert_eq!(ws.log.last().unwrap().actor, Actor::System);
}
