use super::*;

#[tokio::test]
async fn ohne_seed_startet_der_workspace_leer() {
    let conn = db::open_im_speicher().unwrap();
    let (ws, revision) = stand_von(zustand_mit(conn, leer(), Arc::new(MemoryStore::new()))).await;
    assert_eq!(ws, cockpit_core::Workspace::default());
    assert!(ws.items.is_empty() && ws.reviews.is_empty() && ws.runs.is_empty());
    assert_eq!(revision, 0);
}

#[tokio::test]
async fn seed_demo_laedt_die_beispieldaten() {
    let e = Einstellungen {
        seed: Seed::Demo,
        ..leer()
    };
    let conn = db::open_im_speicher().unwrap();
    let (ws, _) = stand_von(zustand_mit(conn, e, Arc::new(MemoryStore::new()))).await;
    assert_eq!(ws, cockpit_core::demo::workspace());
}

#[tokio::test]
async fn ein_gespeicherter_stand_geht_dem_seed_vor() {
    let mut gespeichert = cockpit_core::demo::workspace();
    let id = gespeichert.items[0].id.clone();
    gespeichert.mark_done(&id);
    let text = cockpit_core::persist::save_with_revision(&gespeichert, 5);
    for seed in [Seed::Leer, Seed::Demo] {
        let conn = db::open_im_speicher().unwrap();
        db::workspace_speichern(&conn, 5, &text).unwrap();
        let e = Einstellungen { seed, ..leer() };
        let (ws, revision) = stand_von(zustand_mit(conn, e, Arc::new(MemoryStore::new()))).await;
        assert_eq!(ws, gespeichert, "{seed:?}");
        assert_eq!(revision, 5);
    }
}

#[tokio::test]
async fn unlesbarer_stand_faellt_auf_den_seed_zurueck() {
    let conn = db::open_im_speicher().unwrap();
    db::workspace_speichern(&conn, 7, "{kaputt").unwrap();
    let (ws, revision) = stand_von(zustand_mit(conn, leer(), Arc::new(MemoryStore::new()))).await;
    assert_eq!(ws, cockpit_core::Workspace::default());
    assert_eq!(revision, 0);
}

#[tokio::test]
async fn aktionen_auf_dem_leeren_workspace_lehnen_unbekannte_eintraege_ab() {
    let conn = db::open_im_speicher().unwrap();
    let app = router(
        zustand_mit(conn, leer(), Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (status, body) = schicke(app, &mark_done("gibt-es-nicht")).await;
    assert!(status.is_client_error(), "{status} {body}");
}

#[tokio::test]
async fn health_meldet_den_endpunkt_ohne_den_key() {
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-geheim-123456"))
        .unwrap();
    let e = Einstellungen {
        agent: AgentWahl {
            anbieter: Provider::ClaudeAgentSdk,
            endpunkt: Some(EndpunktKonfig {
                url: Some("http://localhost:4000".into()),
                modell: Some("claude-sonnet-5-5".into()),
            }),
        },
        ..leer()
    };
    let conn = db::open_im_speicher().unwrap();
    let app = router(zustand_mit(conn, e, secrets), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    assert!(!body.contains("geheim"), "{body}");
    let j = json(&body);
    assert_eq!(j["endpunkt"]["konfiguriert"], true);
    assert_eq!(j["endpunkt"]["anbieter"], "claude-agent-sdk");
    assert_eq!(j["endpunkt"]["url"], "http://localhost:4000");
    assert_eq!(j["endpunkt"]["modell"], "claude-sonnet-5-5");
    assert_eq!(j["endpunkt"]["key"], true);

    // Der ältere Schlüsselname `litellm` zählt weiter.
    let alt = Arc::new(MemoryStore::new());
    alt.set(secrets::LITELLM, &Secret::new("sk-alt-123456"))
        .unwrap();
    let app = router(zustand(alt), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    assert_eq!(json(&body)["endpunkt"]["key"], true);

    // Ohne Einstellung: nicht konfiguriert, kein Key.
    let app = router(zustand(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["endpunkt"]["konfiguriert"], false);
    assert_eq!(j["endpunkt"]["key"], false);
    assert_eq!(j["container"]["quelle"], "keine");
    assert_eq!(j["container"]["laufzeit"], serde_json::Value::Null);
}

#[cfg(unix)]
#[tokio::test]
async fn health_meldet_die_container_laufzeit() {
    use std::os::unix::fs::PermissionsExt;
    let ordner = tempfile::tempdir().unwrap();
    let datei = ordner.path().join("podman");
    std::fs::write(&datei, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&datei, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = Einstellungen {
        suchpfad: Some(ordner.path().as_os_str().to_owned()),
        ..Einstellungen::default()
    };
    let conn = db::open_im_speicher().unwrap();
    let app = router(
        zustand_mit(conn, e, Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["container"]["laufzeit"], "podman");
    assert_eq!(j["container"]["engine_laeuft"], true);
    assert_eq!(j["container"]["quelle"], "erkannt");
}
