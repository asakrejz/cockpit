use super::*;

// ---------- GET /api/workspace ----------

#[tokio::test]
async fn workspace_starts_with_the_demo_data() {
    let (status, body) = hole(app_without_dist(), "/api/workspace").await;
    assert_eq!(status, StatusCode::OK);
    let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(ws, cockpit_core::demo::multi_repo_workspace());
    assert_eq!(revision, 0);
}

// ---------- POST /api/actions ----------

#[tokio::test]
async fn action_changes_the_state_and_counts_the_revision() {
    let state = state(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let id = first_id();

    let (status, body) = send(app(), &mark_done(&id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let answer = json(&body);
    assert_eq!(answer["revision"], 1);
    assert_eq!(answer["logged"], 1);
    assert!(!answer["summary"].as_str().unwrap().is_empty());

    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(revision, 1);
    assert!(ws.item(&id).unwrap().read);
    // The server applies as "Du", with its clock.
    let last = ws.log.last().unwrap();
    assert_eq!(last.actor, Actor::User);
    assert!(last.at_ms > 1_600_000_000_000);
}

#[tokio::test]
async fn rule_violation_is_422_with_german_message_and_changes_nothing() {
    let state = state(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let before = hole(app(), "/api/workspace").await.1;

    let (status, body) = send(app(), r#"{"action":{"MarkDone":{"item":"gibt-es-nicht"}}}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json(&body)["error"]
            .as_str()
            .unwrap()
            .contains("gibt es nicht")
    );
    assert_eq!(hole(app(), "/api/workspace").await.1, before);
}

#[tokio::test]
async fn review_send_with_open_suggestions_is_4_xx() {
    let ws = cockpit_core::demo::workspace();
    let number = ws
        .reviews
        .iter()
        .find(|r| r.suggestions.iter().any(|s| s.decision.is_none()))
        .expect("Beispieldaten haben ein Review mit offenen Vorschlägen")
        .number;
    let body = format!(r#"{{"action":{{"SendReview":{{"review":{number},"comment":""}}}}}}"#);
    let (status, answer) = send(app_without_dist(), &body).await;
    assert!(status.is_client_error(), "{status}");
    assert!(
        json(&answer)["error"]
            .as_str()
            .is_some_and(|m| !m.is_empty())
    );
}

#[tokio::test]
async fn merging_only_with_met_conditions() {
    // Via the API every action comes as "Du". That an agent never merges is checked by
    // `Workspace::apply` itself (actor); here the conditions for merging apply.
    let body = r#"{"action":{"MergePr":{"review":87,"method":"Squash"}}}"#;
    let (status, answer) = send(app_without_dist(), body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(!json(&answer)["error"].as_str().unwrap().is_empty());
    // Shapes that do not exist stay 400.
    for body in [
        r#"{"action":{"Merge":{"review":87}}}"#,
        r#"{"action":"MergePullRequest"}"#,
    ] {
        let (status, answer) = send(app_without_dist(), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(json(&answer)["error"].as_str().unwrap().contains("Aktion"));
    }
}

#[tokio::test]
async fn broken_body_is_400() {
    for body in ["", "kein json", "{}", r#"{"action":"GibtEsNicht"}"#] {
        let (status, answer) = send(app_without_dist(), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert!(answer.contains("error"));
    }
}
