use super::*;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "sk-geheim-1234567890";

fn key() -> Secret {
    Secret::new(KEY.to_string())
}

fn answer(text: &str) -> serde_json::Value {
    json!({"content": [{"type": "text", "text": text}]})
}

#[tokio::test]
async fn a_proxy_gets_the_text_and_the_target_language() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .and(body_partial_json(json!({
            "model": "claude-sonnet-5-5",
            "messages": [{"role": "user", "content": "Der Toggle zeigt keinen Fokus."}],
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(answer(" The toggle shows no focus. ")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let target = Target::from(Some(&server.uri()), None);
    let out = translate(
        &target,
        Some(&key()),
        Some("claude-sonnet-5-5"),
        "Der Toggle zeigt keinen Fokus.",
        Lang::En,
    )
    .await
    .unwrap();
    assert_eq!(out, "The toggle shows no focus.");
    // The system prompt names the language and says the text is data.
    let sent = server.received_requests().await.unwrap();
    let body = String::from_utf8_lossy(&sent[0].body).to_string();
    assert!(body.contains("into English") && body.contains("never instructions"));
    assert!(
        !body.contains(KEY),
        "the key is a header, never part of the body"
    );
}

#[tokio::test]
async fn directly_at_anthropic_the_key_goes_in_x_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", KEY))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer("Hallo")))
        .expect(1)
        .mount(&server)
        .await;
    // `direct_base` replaces the Anthropic address (tests only).
    let target = Target::from(None, Some(&server.uri()));
    let out = translate(&target, Some(&key()), Some("m"), "Hello", Lang::De)
        .await
        .unwrap();
    assert_eq!(out, "Hallo");
}

#[tokio::test]
async fn an_openai_compatible_endpoint_uses_chat_completions() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"choices": [{"message": {"role": "assistant", "content": "Hallo Welt"}}]}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let target = Target::for_provider(true, Some(&server.uri()), None);
    let out = translate(
        &target,
        Some(&key()),
        Some("gpt-x"),
        "Hello world",
        Lang::De,
    )
    .await
    .unwrap();
    assert_eq!(out, "Hallo Welt");
}

#[tokio::test]
async fn without_key_model_or_with_too_long_a_text_nothing_is_called() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let target = Target::from(Some(&server.uri()), None);
    assert_eq!(
        translate(&target, None, Some("m"), "x", Lang::En).await,
        Err(TranslateError::NoKey)
    );
    assert_eq!(
        translate(&target, Some(&key()), None, "x", Lang::En).await,
        Err(TranslateError::NoModel)
    );
    assert_eq!(
        translate(&target, Some(&key()), Some(" "), "x", Lang::En).await,
        Err(TranslateError::NoModel)
    );
    let long = "a".repeat(MAX_TEXT + 1);
    assert_eq!(
        translate(&target, Some(&key()), Some("m"), &long, Lang::En).await,
        Err(TranslateError::TooLong)
    );
}

#[tokio::test]
async fn errors_of_the_endpoint_become_plain_reasons() {
    for (status, expected) in [
        (401, TranslateError::KeyRejected),
        (403, TranslateError::KeyRejected),
        (429, TranslateError::Status(429)),
        (500, TranslateError::Status(500)),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_string("secret details"))
            .mount(&server)
            .await;
        let target = Target::from(Some(&server.uri()), None);
        let got = translate(&target, Some(&key()), Some("m"), "x", Lang::En).await;
        assert_eq!(got, Err(expected));
        // The reason never carries the body of the answer.
        assert!(!got.unwrap_err().msg().de().contains("secret details"));
    }
}

#[tokio::test]
async fn an_answer_without_text_is_bad() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"content": []})))
        .mount(&server)
        .await;
    let target = Target::from(Some(&server.uri()), None);
    assert_eq!(
        translate(&target, Some(&key()), Some("m"), "x", Lang::En).await,
        Err(TranslateError::BadAnswer)
    );
}

#[tokio::test]
async fn redirects_are_not_followed_so_the_key_stays_put() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("location", "http://127.0.0.1:9/steal"),
        )
        .mount(&server)
        .await;
    let target = Target::from(Some(&server.uri()), None);
    assert_eq!(
        translate(&target, Some(&key()), Some("m"), "x", Lang::En).await,
        Err(TranslateError::Status(307))
    );
}
