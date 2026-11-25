use super::*;

// ---------- Security ----------

#[tokio::test]
async fn post_without_json_content_type_is_415() {
    for kind in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let mut b = request("POST", "/api/actions");
        if let Some(t) = kind {
            b = b.header("content-type", t);
        }
        let req = b.body(Body::from(mark_done(&first_id()))).unwrap();
        let (status, _) = send_request(app_without_dist(), req).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{kind:?}");
    }
}

#[tokio::test]
async fn post_with_foreign_origin_is_403_and_changes_nothing() {
    let state = state(Arc::new(MemoryStore::new()));
    let app = || router(state.clone(), PathBuf::from("/x"));
    let before = fetch(app(), "/api/workspace").await.1;
    let req = request("POST", "/api/actions")
        .header("content-type", "application/json")
        .header("origin", "https://evil.example")
        .body(Body::from(mark_done(&first_id())))
        .unwrap();
    let (status, _) = send_request(app(), req).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(fetch(app(), "/api/workspace").await.1, before);

    // The same origin works.
    let req = request("POST", "/api/actions")
        .header("content-type", "application/json")
        .header("origin", "http://localhost:8787")
        .body(Body::from(mark_done(&first_id())))
        .unwrap();
    assert_eq!(send_request(app(), req).await.0, StatusCode::OK);
}

#[tokio::test]
async fn foreign_host_is_403_on_all_api_routes() {
    for (method, path) in [
        ("GET", "/api/health"),
        ("GET", "/api/workspace"),
        ("GET", "/api/events"),
        ("POST", "/api/actions"),
    ] {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "evil.example:8787")
            .header("content-type", "application/json")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
            .body(Body::from(mark_done(&first_id())))
            .unwrap();
        let (status, _) = send_request(app_without_dist(), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
    }
}

#[tokio::test]
async fn foreign_peer_is_403() {
    let req = Request::get("/api/workspace")
        .header("host", "127.0.0.1:8787")
        .extension(ConnectInfo(SocketAddr::from(([192, 168, 0, 7], 1))))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send_request(app_without_dist(), req).await.0,
        StatusCode::FORBIDDEN
    );
    // Without a known peer it is rejected, not let through.
    let req = Request::get("/api/workspace")
        .header("host", "127.0.0.1:8787")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send_request(app_without_dist(), req).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn host_is_against_the_configured_port_checked() {
    let state = AppState::with_port(
        db::open_in_memory().unwrap(),
        Arc::new(MemoryStore::new()),
        9000,
    );
    let app = || router(state.clone(), PathBuf::from("/x"));
    assert_eq!(fetch(app(), "/api/health").await.0, StatusCode::FORBIDDEN);
    let req = Request::get("/api/health")
        .header("host", "localhost:9000")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send_request(app(), req).await.0, StatusCode::OK);
}
