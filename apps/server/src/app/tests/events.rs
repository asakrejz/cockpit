use super::*;

#[tokio::test]
async fn ereignisse_melden_revision_und_neue_eintraege() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));

    let antwort = app()
        .oneshot(anfrage("GET", "/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(antwort.status(), StatusCode::OK);
    assert_eq!(
        antwort.headers()[CONTENT_TYPE].to_str().unwrap(),
        "text/event-stream"
    );

    let (status, _) = schicke(app(), &mark_done(&erste_id())).await;
    assert_eq!(status, StatusCode::OK);

    let ereignisse = lese_ereignisse(antwort.into_body(), 2).await;
    // Erst der Stand beim Verbinden, dann die Aktion.
    assert_eq!(ereignisse[0]["revision"], 0);
    assert_eq!(ereignisse[0]["eintraege"], serde_json::json!([]));
    assert_eq!(ereignisse[1]["revision"], 1);
    let eintraege = ereignisse[1]["eintraege"].as_array().unwrap();
    assert_eq!(eintraege.len(), 1);
    assert_eq!(eintraege[0]["actor"], "Du");
}

#[tokio::test]
async fn fehlgeschlagene_aktion_sendet_kein_ereignis() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let antwort = app()
        .oneshot(anfrage("GET", "/api/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (status, _) = schicke(app(), r#"{"action":{"MarkDone":{"item":"nein"}}}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = schicke(app(), &mark_done(&erste_id())).await;
    assert_eq!(status, StatusCode::OK);
    let ereignisse = lese_ereignisse(antwort.into_body(), 2).await;
    assert_eq!(ereignisse[1]["revision"], 1, "{ereignisse:?}");
}
