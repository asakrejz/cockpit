use super::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "sk-geheim-1234567890";

fn models(ids: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "object": "list",
        "data": ids.iter().map(|id| serde_json::json!({"id": id, "object": "model"})).collect::<Vec<_>>(),
    })
}

async fn proxy(response: ResponseTemplate) -> MockServer {
    proxy_n(response, 1).await
}

async fn proxy_n(response: ResponseTemplate, n: u64) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(response)
        .expect(n)
        .mount(&server)
        .await;
    server
}

fn target(server: &MockServer) -> Target {
    Target::from(Some(&server.uri()), None)
}

#[test]
fn target_depends_on_the_url() {
    let z = Target::from(Some(" http://localhost:4000/ "), None);
    assert_eq!(
        (z.base.as_str(), z.direct),
        ("http://localhost:4000", false)
    );
    let z = Target::from(None, None);
    assert_eq!((z.base.as_str(), z.direct), (ANTHROPIC_API, true));
    assert!(Target::from(Some("  "), Some("http://127.0.0.1:9/")).direct);
    assert_eq!(
        Target::from(Some(""), Some("http://127.0.0.1:9/")).base,
        "http://127.0.0.1:9"
    );
}

#[tokio::test]
async fn list_is_sorted_with_claude_first() {
    let server = proxy(ResponseTemplate::new(200).set_body_json(models(&[
        "gpt-x",
        "claude-sonnet-5-5",
        "azure/b",
        "Claude-Haiku",
        "claude-sonnet-5-5",
        "claude-a",
    ])))
    .await;
    let l = models_load(&target(&server), Some(&Secret::new(KEY))).await;
    assert!(l.loaded && l.reachable && l.key_ok);
    assert_eq!(
        l.models,
        [
            "Claude-Haiku",
            "claude-a",
            "claude-sonnet-5-5",
            "azure/b",
            "gpt-x"
        ]
    );
    assert_eq!(l.hint, None);
}

#[tokio::test]
async fn only_ids_come_back_and_odd_entries_are_dropped() {
    let body = serde_json::json!({"data": [
        {"id": "ok-1", "owned_by": "geheim-intern", "pricing": {"x": 1}},
        {"id": ""}, {"id": 7}, {"nichts": true}, {"id": "mit leerzeichen"}, {"id": "x\ny"},
        "kein-objekt",
    ]});
    let server = proxy(ResponseTemplate::new(200).set_body_json(body)).await;
    let l = models_load(&target(&server), Some(&Secret::new(KEY))).await;
    assert_eq!(l.models, ["ok-1"]);
    let text = serde_json::to_string(&l).unwrap();
    assert!(!text.contains("geheim"), "{text}");
}

#[tokio::test]
async fn a_huge_list_is_capped() {
    let ids: Vec<String> = (0..1500).map(|i| format!("modell-{i:04}")).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let server = proxy(ResponseTemplate::new(200).set_body_json(models(&refs))).await;
    let l = models_load(&target(&server), Some(&Secret::new(KEY))).await;
    assert_eq!(l.models.len(), MAX_MODELS);
    assert_eq!(l.models[0], "modell-0000");
}

#[tokio::test]
async fn a_response_beyond_the_size_limit_is_refused() {
    let gross = "x".repeat(MAX_RESPONSE + 10);
    let server = proxy(ResponseTemplate::new(200).set_body_string(gross)).await;
    let l = models_load(&target(&server), Some(&Secret::new(KEY))).await;
    assert!(!l.loaded && l.models.is_empty());
    assert!(l.hint.unwrap().contains("kein Modellverzeichnis"));
}

#[tokio::test]
async fn rejected_key_empty_list_and_broken_json_say_so() {
    let server = proxy(ResponseTemplate::new(401)).await;
    let l = models_load(&target(&server), Some(&Secret::new(KEY))).await;
    assert!(l.reachable && !l.key_ok && !l.loaded);
    assert!(l.hint.unwrap().contains("lehnt den Key ab"));

    let server = proxy(ResponseTemplate::new(200).set_body_json(models(&[]))).await;
    let l = models_load(&target(&server), Some(&Secret::new(KEY))).await;
    assert!(l.reachable && l.key_ok && !l.loaded && l.models.is_empty());
    assert!(l.hint.unwrap().contains("keine Modelle"));

    for broken in ["<html>", "{\"data\": 5}", "{\"modelle\": []}", ""] {
        let server = proxy(ResponseTemplate::new(200).set_body_string(broken)).await;
        let l = models_load(&target(&server), Some(&Secret::new(KEY))).await;
        assert!(!l.loaded && l.reachable, "{broken}");
        assert!(
            l.hint.unwrap().contains("kein Modellverzeichnis"),
            "{broken}"
        );
    }

    let server = proxy(ResponseTemplate::new(500)).await;
    let l = models_load(&target(&server), Some(&Secret::new(KEY))).await;
    assert!(l.hint.unwrap().contains("500"));
}

#[test]
fn openai_target_goes_directly_to_openai() {
    let z = Target::for_provider(true, None, None);
    assert_eq!(
        (z.base.as_str(), z.direct, z.openai),
        (OPENAI_API, true, true)
    );
    let z = Target::for_provider(true, Some("http://p:1/"), None);
    assert_eq!((z.base.as_str(), z.direct), ("http://p:1", false));
    assert!(!Target::for_provider(false, None, None).openai);
}

#[tokio::test]
async fn codex_calls_use_the_bearer_key_even_directly_and_name_openai() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(models(&["gpt-5"])))
        .expect(2)
        .mount(&server)
        .await;
    let z = Target::for_provider(true, None, Some(&server.uri()));
    let l = models_load(&z, Some(&Secret::new(KEY))).await;
    assert!(l.loaded && l.models == ["gpt-5"], "{l:?}");
    let p = verify(&z, Some(&Secret::new(KEY)), Some("gpt-5")).await;
    assert!(p.model_present);
    // The texts name OpenAI, not Anthropic.
    let free = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let z = Target::for_provider(true, None, Some(&format!("http://127.0.0.1:{free}")));
    let p = verify(&z, Some(&Secret::new(KEY)), Some("m")).await;
    assert!(p.hint.unwrap().contains("OpenAI"));
}

#[tokio::test]
async fn direct_calls_use_x_api_key_and_the_version_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", KEY))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(models(&["claude-opus-5-5"])))
        .expect(1)
        .mount(&server)
        .await;
    let z = Target::from(None, Some(&server.uri()));
    let l = models_load(&z, Some(&Secret::new(KEY))).await;
    assert_eq!(l.models, ["claude-opus-5-5"]);
    // Never a Bearer header on the direct call.
    let request = server.received_requests().await.unwrap();
    assert!(request[0].headers.get("authorization").is_none());
    assert_eq!(request[0].url.query(), Some("limit=1000"));
}

#[tokio::test]
async fn check_reports_reachable_key_and_model() {
    let server = proxy(
        ResponseTemplate::new(200)
            .set_body_json(models(&["claude-sonnet-5-5", "claude-haiku-5-5"])),
    )
    .await;
    let p = verify(
        &target(&server),
        Some(&Secret::new(KEY)),
        Some("claude-sonnet-5-5"),
    )
    .await;
    assert!(p.configured && p.reachable && p.key_ok && p.model_present);
    assert_eq!(p.hint, None);
}

#[tokio::test]
async fn missing_model_names_the_offered_ones() {
    let server =
        proxy(ResponseTemplate::new(200).set_body_json(models(&["gpt-x", "claude-a"]))).await;
    let z = Target::from(Some(&format!("{}/", server.uri())), None);
    let p = verify(&z, Some(&Secret::new(KEY)), Some("claude-sonnet-5-5")).await;
    assert!(p.reachable && p.key_ok && !p.model_present);
    let hint = p.hint.unwrap();
    assert!(
        hint.contains("gpt-x") && hint.contains("claude-a"),
        "{hint}"
    );
}

#[tokio::test]
async fn check_without_a_model_gives_a_hint() {
    let server = proxy(ResponseTemplate::new(200).set_body_json(models(&["a"]))).await;
    let p = verify(&target(&server), Some(&Secret::new(KEY)), None).await;
    assert!(p.key_ok && !p.model_present);
    assert!(p.hint.unwrap().contains("Kein Modell"));
}

#[tokio::test]
async fn check_with_a_wrong_key_is_reachable_but_not_ok() {
    let server = proxy(ResponseTemplate::new(401)).await;
    let p = verify(&target(&server), Some(&Secret::new(KEY)), Some("m")).await;
    assert!(p.reachable && !p.key_ok && !p.model_present);
    assert!(p.hint.unwrap().contains("Key"));
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let target_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(models(&["m"])))
        .expect(0)
        .mount(&target_server)
        .await;
    let server = proxy_n(
        ResponseTemplate::new(302)
            .insert_header("location", format!("{}/v1/models", target_server.uri())),
        2,
    )
    .await;
    let p = verify(&target(&server), Some(&Secret::new(KEY)), Some("m")).await;
    assert!(p.reachable && !p.key_ok);
    let l = models_load(&target(&server), Some(&Secret::new(KEY))).await;
    assert!(!l.loaded);
}

#[tokio::test]
async fn unreachable_endpoint() {
    // A port nobody is listening on.
    let free = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let z = Target::from(Some(&format!("http://127.0.0.1:{free}")), None);
    let p = verify(&z, Some(&Secret::new(KEY)), Some("m")).await;
    assert!(p.configured && !p.reachable && !p.key_ok);
    assert!(p.hint.unwrap().contains("nicht erreichbar"));
    let l = models_load(&z, Some(&Secret::new(KEY))).await;
    assert!(!l.reachable && !l.loaded);
}

#[tokio::test]
async fn without_a_key_nothing_is_called_and_the_key_never_shows_up() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let p = verify(&target(&server), None, Some("m")).await;
    assert!(p.configured && !p.reachable);
    assert!(p.hint.as_deref().unwrap().contains("agent-key"));
    let l = models_load(&target(&server), Some(&Secret::new(""))).await;
    assert!(!l.loaded && l.hint.unwrap().contains("kein Key"));

    let server = proxy(ResponseTemplate::new(401)).await;
    let p = verify(&target(&server), Some(&Secret::new(KEY)), Some("m")).await;
    let l = models_load(&target(&second_server().await), Some(&Secret::new(KEY))).await;
    let text = format!(
        "{}{}",
        serde_json::to_string(&p).unwrap(),
        serde_json::to_string(&l).unwrap()
    );
    assert!(!text.contains(KEY) && !text.contains("geheim"), "{text}");
}

async fn second_server() -> MockServer {
    proxy(ResponseTemplate::new(500)).await
}
