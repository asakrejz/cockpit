use super::*;

// ---------- Sicherheit ----------

#[tokio::test]
async fn post_ohne_json_content_type_ist_415() {
    for typ in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let mut b = anfrage("POST", "/api/actions");
        if let Some(t) = typ {
            b = b.header("content-type", t);
        }
        let req = b.body(Body::from(mark_done(&erste_id()))).unwrap();
        let (status, _) = sende(app_ohne_dist(), req).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{typ:?}");
    }
}

#[tokio::test]
async fn post_mit_fremder_herkunft_ist_403_und_aendert_nichts() {
    let state = zustand(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let vorher = hole(app(), "/api/workspace").await.1;
    let req = anfrage("POST", "/api/actions")
        .header("content-type", "application/json")
        .header("origin", "https://evil.example")
        .body(Body::from(mark_done(&erste_id())))
        .unwrap();
    let (status, _) = sende(app(), req).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(hole(app(), "/api/workspace").await.1, vorher);

    // Gleiche Herkunft geht.
    let req = anfrage("POST", "/api/actions")
        .header("content-type", "application/json")
        .header("origin", "http://localhost:8787")
        .body(Body::from(mark_done(&erste_id())))
        .unwrap();
    assert_eq!(sende(app(), req).await.0, StatusCode::OK);
}

#[tokio::test]
async fn fremder_host_ist_403_auf_allen_api_routen() {
    for (methode, pfad) in [
        ("GET", "/api/health"),
        ("GET", "/api/workspace"),
        ("GET", "/api/events"),
        ("POST", "/api/actions"),
    ] {
        let req = Request::builder()
            .method(methode)
            .uri(pfad)
            .header("host", "evil.example:8787")
            .header("content-type", "application/json")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
            .body(Body::from(mark_done(&erste_id())))
            .unwrap();
        let (status, _) = sende(app_ohne_dist(), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{methode} {pfad}");
    }
}

#[tokio::test]
async fn fremde_gegenstelle_ist_403() {
    let req = Request::get("/api/workspace")
        .header("host", "127.0.0.1:8787")
        .extension(ConnectInfo(SocketAddr::from(([192, 168, 0, 7], 1))))
        .body(Body::empty())
        .unwrap();
    assert_eq!(sende(app_ohne_dist(), req).await.0, StatusCode::FORBIDDEN);
    // Ohne bekannte Gegenstelle wird abgewiesen, nicht durchgelassen.
    let req = Request::get("/api/workspace")
        .header("host", "127.0.0.1:8787")
        .body(Body::empty())
        .unwrap();
    assert_eq!(sende(app_ohne_dist(), req).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn host_wird_gegen_den_konfigurierten_port_geprueft() {
    let state = AppState::mit_port(
        db::open_im_speicher().unwrap(),
        Arc::new(MemoryStore::new()),
        9000,
    );
    let app = || router(state.clone(), PathBuf::from("/x"));
    assert_eq!(hole(app(), "/api/health").await.0, StatusCode::FORBIDDEN);
    let req = Request::get("/api/health")
        .header("host", "localhost:9000")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
        .body(Body::empty())
        .unwrap();
    assert_eq!(sende(app(), req).await.0, StatusCode::OK);
}
