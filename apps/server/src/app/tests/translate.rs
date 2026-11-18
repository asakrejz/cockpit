use super::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn post(app: Router, text: &str, to: &str) -> (StatusCode, String) {
    let body = serde_json::json!({"text": text, "to": to}).to_string();
    let req = request("POST", "/api/translate")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    send_request(app, req).await
}

fn key(secrets: &Arc<MemoryStore>) {
    secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-geheim-123456"))
        .unwrap();
}

#[tokio::test]
async fn a_text_is_translated_with_the_model_of_the_agent_and_then_comes_from_the_cache() {
    let proxy = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("authorization", "Bearer sk-geheim-123456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "content": [{"type": "text", "text": "The ring is barely visible."}]
        })))
        // The same text asked twice reaches the model once.
        .expect(1)
        .mount(&proxy)
        .await;
    let secrets = Arc::new(MemoryStore::new());
    key(&secrets);
    let app = with_endpoint(Some(&proxy.uri()), None, secrets);
    let text = "Der Ring ist kaum sichtbar (einmaliger Testtext 7731).";
    for _ in 0..2 {
        let (status, body) = post(app.clone(), text, "en").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(!body.contains("geheim"));
        assert_eq!(json(&body)["text"], "The ring is barely visible.");
    }
}

#[tokio::test]
async fn without_a_key_the_endpoint_is_not_called_and_the_reason_comes_as_a_code() {
    let proxy = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&proxy)
        .await;
    let app = with_endpoint(Some(&proxy.uri()), None, Arc::new(MemoryStore::new()));
    let (status, body) = post(app, "Einmaliger Testtext 5521 ohne Schlüssel", "en").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json(&body)["msg"]["code"], "integrations.translate_no_key");
}

#[tokio::test]
async fn an_unreadable_request_and_foreign_hosts_are_refused() {
    let app = with_endpoint(None, None, Arc::new(MemoryStore::new()));
    let req = request("POST", "/api/translate")
        .header("content-type", "application/json")
        .body(Body::from("nonsense"))
        .unwrap();
    let (status, body) = send_request(app.clone(), req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        json(&body)["msg"]["code"],
        "server_misc.translate_unreadable"
    );
    // From another host the access check answers before the handler runs.
    let req = Request::builder()
        .method("POST")
        .uri("/api/translate")
        .header("host", "evil.example")
        .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))))
        .body(Body::from("{}"))
        .unwrap();
    let (status, _) = send_request(app, req).await;
    assert_ne!(status, StatusCode::OK);
}
