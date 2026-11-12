use super::*;

#[tokio::test]
async fn pruefen_meldet_erreichbar_key_und_modell_unter_beiden_adressen() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-geheim-123456"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [{"id": "claude-sonnet-5-5"}]})),
        )
        .expect(2)
        .mount(&proxy)
        .await;
    // Der Key liegt unter dem älteren Namen `litellm`.
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::LITELLM, &Secret::new("sk-geheim-123456"))
        .unwrap();
    for pfad in ["/api/agent/pruefen", "/api/litellm/pruefen"] {
        let (status, body) = hole(
            mit_endpunkt(Some(&proxy.uri()), None, secrets.clone()),
            pfad,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("geheim"), "{body}");
        let j = json(&body);
        assert_eq!(j["konfiguriert"], true, "{pfad}");
        assert_eq!(j["erreichbar"], true);
        assert_eq!(j["key_ok"], true);
        assert_eq!(j["modell_vorhanden"], true);
    }
}

#[tokio::test]
async fn pruefen_ohne_key_ruft_den_endpunkt_nicht_auf() {
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&proxy)
        .await;
    let (_, body) = hole(
        mit_endpunkt(Some(&proxy.uri()), None, Arc::new(MemoryStore::new())),
        "/api/agent/pruefen",
    )
    .await;
    let j = json(&body);
    assert_eq!(j["konfiguriert"], true);
    assert_eq!(j["key_ok"], false);
    assert!(j["hinweis"].as_str().unwrap().contains("agent-key"));
}

#[tokio::test]
async fn pruefen_ohne_einstellung_und_von_fremden_hosts() {
    let app = router(zustand(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
    for pfad in ["/api/agent/pruefen", "/api/litellm/pruefen"] {
        let (status, body) = hole(app.clone(), pfad).await;
        assert_eq!(status, StatusCode::OK);
        let j = json(&body);
        assert_eq!(j["konfiguriert"], false);
        assert!(j["hinweis"].as_str().unwrap().contains("Modell"));
    }
    for pfad in ["/api/agent/modelle", "/api/litellm/pruefen"] {
        let req = Request::builder()
            .uri(pfad)
            .header("host", "boese.example")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
            .body(Body::empty())
            .unwrap();
        assert_eq!(sende(app.clone(), req).await.0, StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn modellliste_vom_proxy_mit_bearer_und_ohne_den_key() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer sk-geheim-123456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"data": [{"id": "gpt-x"}, {"id": "claude-b"}, {"id": "claude-a"}]}),
        ))
        .expect(1)
        .mount(&proxy)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-geheim-123456"))
        .unwrap();
    let (status, body) = hole(
        mit_endpunkt(Some(&proxy.uri()), None, secrets),
        "/api/agent/modelle",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("geheim"), "{body}");
    let j = json(&body);
    assert_eq!(j["geladen"], true);
    assert_eq!(
        j["modelle"],
        serde_json::json!(["claude-a", "claude-b", "gpt-x"])
    );
}

#[tokio::test]
async fn modellliste_direkt_nimmt_x_api_key() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let anthropic = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", "sk-ant-geheim"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [{"id": "claude-opus-5-5"}]})),
        )
        .expect(1)
        .mount(&anthropic)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-ant-geheim"))
        .unwrap();
    let (_, body) = hole(
        mit_endpunkt(None, Some(&anthropic.uri()), secrets),
        "/api/agent/modelle",
    )
    .await;
    assert!(!body.contains("geheim"), "{body}");
    assert_eq!(
        json(&body)["modelle"],
        serde_json::json!(["claude-opus-5-5"])
    );
}

#[tokio::test]
async fn modellliste_meldet_abgelehnten_key_und_fehlenden_key_in_klartext() {
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&proxy)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-falsch-1"))
        .unwrap();
    let (_, body) = hole(
        mit_endpunkt(Some(&proxy.uri()), None, secrets),
        "/api/agent/modelle",
    )
    .await;
    let j = json(&body);
    assert_eq!(j["geladen"], false);
    assert_eq!(j["erreichbar"], true);
    assert!(j["hinweis"].as_str().unwrap().contains("lehnt den Key ab"));
    assert!(!body.contains("falsch"), "{body}");

    // Ohne Key wird nichts aufgerufen.
    let still = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&still)
        .await;
    let (_, body) = hole(
        mit_endpunkt(Some(&still.uri()), None, Arc::new(MemoryStore::new())),
        "/api/agent/modelle",
    )
    .await;
    assert!(
        json(&body)["hinweis"]
            .as_str()
            .unwrap()
            .contains("kein Key")
    );
}

#[tokio::test]
async fn health_meldet_den_agenten_nie_geheimnisse() {
    // Simuliert (Tests starten mit dem Seed demo): immer bereit.
    let app = router(zustand(Arc::new(MemoryStore::new())), PathBuf::from("/x"));
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["agent"]["modus"], "simuliert");
    assert_eq!(j["agent"]["bereit"], true);
    // Echt, aber nichts eingerichtet: sagt in Klartext, was fehlt.
    let conn = db::open_im_speicher().unwrap();
    let e = Einstellungen {
        seed: Seed::Leer,
        ..leer()
    };
    let app = router(
        zustand_mit(conn, e, Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (_, body) = hole(app, "/api/health").await;
    let j = json(&body);
    assert_eq!(j["agent"]["modus"], "echt");
    assert_eq!(j["agent"]["bereit"], false);
    assert_eq!(j["agent"]["image_name"], "localhost/cockpit-agent:1");
    let fehlt = j["agent"]["fehlt"].as_array().unwrap();
    assert!(fehlt.len() >= 3);
    assert!(
        fehlt
            .iter()
            .all(|f| f.as_str().unwrap().contains("Einrichten"))
    );
}

#[tokio::test]
async fn image_bauen_ohne_laufzeit_ist_422_mit_klartext() {
    let conn = db::open_im_speicher().unwrap();
    let e = Einstellungen {
        seed: Seed::Leer,
        ..leer()
    };
    let app = router(
        zustand_mit(conn, e, Arc::new(MemoryStore::new())),
        PathBuf::from("/x"),
    );
    let (status, body) = hole(app.clone(), "/api/agent/image").await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&body);
    assert_eq!(j["laeuft"], false);
    assert_eq!(j["vorhanden"], false);
    let req = anfrage("POST", "/api/agent/image")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let (status, body) = sende(app, req).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json(&body)["fehler"]
            .as_str()
            .unwrap()
            .contains("Rancher Desktop"),
        "{body}"
    );
}
