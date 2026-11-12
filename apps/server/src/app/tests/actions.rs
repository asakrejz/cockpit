use super::*;

// ---------- GET /api/workspace ----------

#[tokio::test]
async fn workspace_startet_mit_den_demo_daten() {
    let (status, body) = hole(app_ohne_dist(), "/api/workspace").await;
    assert_eq!(status, StatusCode::OK);
    let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(ws, cockpit_core::demo::workspace());
    assert_eq!(revision, 0);
}

// ---------- POST /api/actions ----------

#[tokio::test]
async fn aktion_aendert_den_stand_und_zaehlt_die_revision() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let id = erste_id();

    let (status, body) = schicke(app(), &mark_done(&id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let antwort = json(&body);
    assert_eq!(antwort["revision"], 1);
    assert_eq!(antwort["protokolliert"], 1);
    assert!(!antwort["zusammenfassung"].as_str().unwrap().is_empty());

    let (_, body) = hole(app(), "/api/workspace").await;
    let (ws, revision) = cockpit_core::persist::load_with_revision(&body).unwrap();
    assert_eq!(revision, 1);
    assert!(ws.item(&id).unwrap().read);
    // Der Server wendet als „Du“ an, mit seiner Uhr.
    let letzter = ws.log.last().unwrap();
    assert_eq!(letzter.actor, Actor::User);
    assert!(letzter.at_ms > 1_600_000_000_000);
}

#[tokio::test]
async fn regelverstoss_ist_422_mit_deutscher_meldung_und_aendert_nichts() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let vorher = hole(app(), "/api/workspace").await.1;

    let (status, body) =
        schicke(app(), r#"{"action":{"MarkDone":{"item":"gibt-es-nicht"}}}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json(&body)["fehler"]
            .as_str()
            .unwrap()
            .contains("gibt es nicht")
    );
    assert_eq!(hole(app(), "/api/workspace").await.1, vorher);
}

#[tokio::test]
async fn review_senden_mit_offenen_vorschlaegen_ist_4xx() {
    let ws = cockpit_core::demo::workspace();
    let number = ws
        .reviews
        .iter()
        .find(|r| r.suggestions.iter().any(|s| s.decision.is_none()))
        .expect("Beispieldaten haben ein Review mit offenen Vorschlägen")
        .number;
    let body = format!(r#"{{"action":{{"SendReview":{{"review":{number},"comment":""}}}}}}"#);
    let (status, antwort) = schicke(app_ohne_dist(), &body).await;
    assert!(status.is_client_error(), "{status}");
    assert!(
        json(&antwort)["fehler"]
            .as_str()
            .is_some_and(|m| !m.is_empty())
    );
}

#[tokio::test]
async fn mergen_nur_mit_erfuellten_bedingungen() {
    // Über die API kommt jede Aktion als „Du“. Dass ein Agent nie merged, prüft
    // `Workspace::apply` selbst (Actor); hier greifen die Bedingungen fürs Mergen.
    let body = r#"{"action":{"MergePr":{"review":87,"method":"Squash"}}}"#;
    let (status, antwort) = schicke(app_ohne_dist(), body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{antwort}");
    assert!(!json(&antwort)["fehler"].as_str().unwrap().is_empty());
    // Formen, die es nicht gibt, bleiben 400.
    for body in [
        r#"{"action":{"Merge":{"review":87}}}"#,
        r#"{"action":"MergePullRequest"}"#,
    ] {
        let (status, antwort) = schicke(app_ohne_dist(), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            json(&antwort)["fehler"]
                .as_str()
                .unwrap()
                .contains("Aktion")
        );
    }
}

#[tokio::test]
async fn kaputter_body_ist_400() {
    for body in ["", "kein json", "{}", r#"{"action":"GibtEsNicht"}"#] {
        let (status, antwort) = schicke(app_ohne_dist(), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert!(antwort.contains("fehler"));
    }
}
