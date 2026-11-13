use super::*;

#[tokio::test]
async fn check_out_the_container_names_vpn_when_only_the_mac_the_proxy_reached() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data":[{"id":"claude-sonnet-5-5"}]})),
        )
        .mount(&proxy)
        .await;
    let w = welt(true);
    // The proxy of the test is on the Mac; the "container" does not reach it.
    let e = Settings {
        agent: AgentChoice {
            provider: Provider::ClaudeAgentSdk,
            endpoint: Some(EndpointConfig {
                url: Some(proxy.uri()),
                model: Some("claude-sonnet-5-5".into()),
            }),
        },
        ..w.state.inner.settings.clone()
    };
    let conn = crate::db::open(&w.tmp.path().join("p.db")).unwrap();
    let state = AppState::create(
        conn,
        w.secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        e,
    );
    w.write(
        "scenario-plan.sh",
        r#"printf '%s\n' '{"t":"check","reachable":false,"key_ok":false,"hint":"Der Container erreicht den Proxy nicht (ENOTFOUND)."}'
exit 1
"#,
    );
    let p = state.container_check().await;
    assert!(p.checked && p.mac_reached && !p.container_reached, "{p:?}");
    let h = p.hint.unwrap();
    assert!(
        h.contains("Dein Mac erreicht den Proxy, der Container aber nicht")
            && h.contains("VPN")
            && h.contains("ENOTFOUND"),
        "{h}"
    );
    let calls = w.log("calls.log");
    let z = calls.lines().find(|z| z.starts_with("run ")).unwrap();
    assert!(z.contains("--rm") && z.contains(":ro"), "{z}");
    assert!(!calls.contains(TOKEN));
    // If it also works in the container, everything is fine.
    w.write(
        "scenario-plan.sh",
        "printf '%s\\n' '{\"t\":\"check\",\"reachable\":true,\"key_ok\":true,\"hint\":\"ok\"}'\nexit 0\n",
    );
    let p = state.container_check().await;
    assert!(p.container_reached && p.key_ok);
    assert!(
        p.hint
            .unwrap()
            .contains("auch aus dem Container erreichbar")
    );
}

#[tokio::test]
async fn check_out_the_container_without_image_or_key_calls_nothing_on() {
    let w = welt(false);
    let p = w.state.container_check().await;
    assert!(!p.checked && p.hint.unwrap().contains("nicht eingerichtet"));
    let w = welt(true);
    std::fs::write(w.bin().join("no-image"), "").unwrap();
    let p = w.state.container_check().await;
    assert!(!p.checked && p.hint.unwrap().contains("Image fehlt"));
    assert!(!w.log("calls.log").contains("run "));
}

#[tokio::test]
async fn check_out_the_container_for_codex_uses_the_openai_names() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    // Codex talks to an OpenAI-compatible endpoint: always the bearer key.
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data":[{"id":"gpt-test"}]})),
        )
        .mount(&proxy)
        .await;
    let w = welt_with(true, Provider::Codex);
    let e = Settings {
        agent: AgentChoice {
            provider: Provider::Codex,
            endpoint: Some(EndpointConfig {
                url: Some(proxy.uri()),
                model: Some("gpt-test".into()),
            }),
        },
        ..w.state.inner.settings.clone()
    };
    let conn = crate::db::open(&w.tmp.path().join("c.db")).unwrap();
    let state = AppState::create(
        conn,
        w.secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        e,
    );
    w.write(
        "scenario-plan.sh",
        "printf '%s\\n' '{\"t\":\"check\",\"reachable\":true,\"key_ok\":true,\"hint\":\"ok\"}'\nexit 0\n",
    );
    let p = state.container_check().await;
    assert!(
        p.checked && p.mac_reached && p.container_reached && p.key_ok,
        "{p:?}"
    );
    let env = w.log("env.log");
    assert!(
        env.contains("PROVIDER=codex")
            && env.contains(&format!("OAIKEY={TOKEN}"))
            && env.contains("CMODEL=gpt-test")
            && env.contains("TOKEN= "),
        "{env}"
    );
    assert!(!w.log("calls.log").contains(TOKEN));
}
