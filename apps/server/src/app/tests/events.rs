use super::*;

#[tokio::test]
async fn events_report_revision_and_new_entries() {
    let state = state(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));

    let answer = app()
        .oneshot(request("GET", "/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(answer.status(), StatusCode::OK);
    assert_eq!(
        answer.headers()[CONTENT_TYPE].to_str().unwrap(),
        "text/event-stream"
    );

    let (status, _) = send(app(), &mark_done(&first_id())).await;
    assert_eq!(status, StatusCode::OK);

    let events = read_events(answer.into_body(), 2).await;
    // First the state on connecting, then the action.
    assert_eq!(events[0]["revision"], 0);
    assert_eq!(events[0]["entries"], serde_json::json!([]));
    assert_eq!(events[1]["revision"], 1);
    let entries = events[1]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["actor"], "User");
}

#[tokio::test]
async fn failed_action_sends_no_event() {
    let state = state(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let answer = app()
        .oneshot(request("GET", "/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (status, _) = send(app(), r#"{"action":{"MarkDone":{"item":"nein"}}}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = send(app(), &mark_done(&first_id())).await;
    assert_eq!(status, StatusCode::OK);
    let events = read_events(answer.into_body(), 2).await;
    assert_eq!(events[1]["revision"], 1, "{events:?}");
}
