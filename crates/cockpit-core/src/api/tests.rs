use super::*;

#[test]
fn request_has_the_documented_form() {
    let json = serde_json::to_string(&ActionRequest {
        action: Action::AdvanceAgents,
    })
    .unwrap();
    assert_eq!(json, r#"{"action":"AdvanceAgents"}"#);
    let back: ActionRequest =
        serde_json::from_str(r#"{"action":{"MarkDone":{"item":"bau142"}}}"#).unwrap();
    assert_eq!(
        back.action,
        Action::MarkDone {
            item: "bau142".into()
        }
    );
}

#[test]
fn event_without_entries_is_readable() {
    let e: ServerEvent = serde_json::from_str(r#"{"revision":3}"#).unwrap();
    assert_eq!(e.revision, 3);
    assert!(e.entries.is_empty());
}

#[test]
fn setup_statuses_read_old_and_partial_json() {
    assert_eq!(
        serde_json::from_str::<EndpointStatus>("{}").unwrap(),
        EndpointStatus::default()
    );
    assert_eq!(
        serde_json::from_str::<ContainerStatus>("{}").unwrap(),
        ContainerStatus::default()
    );
    assert_eq!(
        serde_json::from_str::<Setup>("{}").unwrap(),
        Setup::default()
    );
    let p: EndpointCheck =
        serde_json::from_str(r#"{"konfiguriert":true,"erreichbar":true}"#).unwrap();
    assert!(p.reachable && !p.key_ok && !p.model_present);
}

#[test]
fn key_input_never_shows_its_value() {
    let e = KeyInput {
        value: "test-wert-123".into(),
    };
    assert!(!format!("{e:?}").contains("test-wert-123"));
}

#[test]
fn config_change_only_sends_touched_sections() {
    let a = ConfigChange {
        github: Some(GitHubInput {
            repos: vec!["firma/app".into()],
        }),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        r#"{"github":{"repos":["firma/app"]}}"#
    );
    let back: ConfigChange = serde_json::from_str("{}").unwrap();
    assert_eq!(back, ConfigChange::default());
}

#[test]
fn providers_have_stable_names_and_only_the_sdk_is_implemented() {
    for a in Provider::ALL {
        assert_eq!(Provider::from_id(a.id()), Some(a));
        let j = serde_json::to_string(&a).unwrap();
        assert_eq!(j, format!("\"{}\"", a.id()));
    }
    assert_eq!(Provider::default(), Provider::ClaudeAgentSdk);
    assert_eq!(Provider::from_id(" Codex "), Some(Provider::Codex));
    assert_eq!(Provider::from_id("gpt"), None);
    assert!(Provider::ClaudeAgentSdk.implemented());
    assert!(Provider::ClaudeAgentSdk.not_implemented().is_none());
    for a in [Provider::ClaudeCode, Provider::Codex] {
        assert!(
            a.not_implemented()
                .unwrap()
                .contains("noch nicht umgesetzt")
        );
    }
}

#[test]
fn the_old_key_name_counts_as_the_agent_key() {
    let with = |n: &str| Setup {
        key: vec![KeyResponse {
            name: n.into(),
            set: true,
        }],
        ..Default::default()
    };
    assert!(with("agent-key").has_key("agent-key"));
    assert!(with("litellm").has_key("agent-key"));
    assert!(!with("agent-key").has_key("litellm"));
    assert!(!Setup::default().has_key("agent-key"));
}

#[test]
fn old_setup_names_still_read() {
    let e: Setup =
        serde_json::from_str(r#"{"litellm_url":"http://x:1","litellm_modell":"m"}"#).unwrap();
    assert_eq!(e.endpoint_url.as_deref(), Some("http://x:1"));
    assert_eq!(e.endpoint_model.as_deref(), Some("m"));
    let k: ConfigChange = serde_json::from_str(r#"{"litellm":{"url":"http://x:1"}}"#).unwrap();
    assert_eq!(k.endpoint.unwrap().url.as_deref(), Some("http://x:1"));
}
