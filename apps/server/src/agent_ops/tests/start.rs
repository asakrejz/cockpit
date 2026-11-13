use super::*;

#[tokio::test]
async fn without_setup_starts_no_run_and_the_message_says_where() {
    let w = welt(false);
    std::fs::remove_file(w.bin().join("docker")).unwrap();
    let st = w.state.clone();
    let status = tokio::task::spawn_blocking(move || st.agent_status())
        .await
        .unwrap();
    assert_eq!(status.mode, "real");
    assert!(!status.ready);
    let text = status.missing.join(" ");
    assert!(
        text.contains("Einrichten")
            && text.contains("Container")
            && text.contains("Modell")
            && text.contains("GitHub"),
        "{text}"
    );
    let error = w
        .action(Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions::default(),
        })
        .await
        .unwrap_err();
    assert!(
        error.contains("Der Agent kann noch nicht starten") && error.contains("Einrichten"),
        "{error}"
    );
    assert!(w.run().is_idle(), "es wurde nichts gestartet");
    // Folgeaufträge gibt es noch nicht mit dem echten Agenten.
    let f = w
        .action(Action::InvestigateCheck {
            review: 85,
            check: "Lint".into(),
        })
        .await
        .unwrap_err();
    assert!(f.contains("noch nicht mit dem echten Agenten"), "{f}");
}

#[tokio::test]
async fn missing_image_is_reported() {
    let w = welt(true);
    std::fs::write(w.bin().join("no-image"), "").unwrap();
    let st = w.state.clone();
    let status = tokio::task::spawn_blocking(move || st.agent_status())
        .await
        .unwrap();
    assert!(!status.ready && !status.image);
    assert!(
        status.missing[0].contains("Image bauen"),
        "{:?}",
        status.missing
    );
}

#[tokio::test]
async fn a_run_from_the_approval_until_to_push() {
    let w = welt(true);
    w.write("scenario-run.sh", &scenario_ok());
    w.start(false).await;
    w.wait("Lauf fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;

    let run = w.run();
    assert!(
        run.summary.contains("Branch claude/BAU-142") && run.summary.contains("gepusht"),
        "{}",
        run.summary
    );
    assert_eq!(run.files, ["neu.txt"]);
    // Verbrauch aus dem Lauf, mit den Preisen aus den Einstellungen.
    let u = run.usage();
    assert_eq!(
        (u.input_tokens, u.output_tokens, u.millis),
        (1000, 200, 1500)
    );
    assert!(u.cost_cents >= 1);
    assert_eq!(w.ws().spent_month(now_ms()).input_tokens, 1000);
    // Protokoll: vom Agenten, in Klartext.
    let agent_entries: Vec<String> = w
        .ws()
        .log_entries(Some(Actor::Agent))
        .iter()
        .map(|e| e.text.clone())
        .collect();
    assert!(
        agent_entries
            .iter()
            .any(|t| t.contains("Container gestartet")),
        "{agent_entries:?}"
    );
    assert!(
        agent_entries.iter().any(|t| t.contains("Datei ändern")),
        "{agent_entries:?}"
    );
    // Der Branch liegt im Remote, main ist unberührt.
    assert_eq!(w.branches(), [BRANCH, "main"]);
    // Nur CLI-Aufrufe, gemeinsame Schalter, Label je Lauf, Zeitlimit-Name, kein Geheimnis.
    let calls = w.log("calls.log");
    let run_line = calls
        .lines()
        .find(|z| z.starts_with("run "))
        .expect("run aufgerufen");
    for muss in [
        "--rm",
        "--label cockpit.run=BAU-142",
        "--add-host cockpit-host:host-gateway",
        ":/work ",
        ":/job:ro",
        "-e ANTHROPIC_AUTH_TOKEN",
        "localhost/cockpit-agent:2",
    ] {
        assert!(run_line.contains(muss), "{muss} fehlt: {run_line}");
    }
    assert!(
        !calls.contains(TOKEN) && !calls.contains(GITHUB),
        "Geheimnis in der Argumentliste: {calls}"
    );
    assert!(
        !calls.contains("https://litellm.example"),
        "auch die Adresse steht nur in der Umgebung"
    );
    // Der Wert kommt über die Umgebung des Kindprozesses an.
    let env = w.log("env.log");
    assert!(
        env.contains(&format!("TOKEN={TOKEN}"))
            && env.contains("BASE=https://litellm.example")
            && env.contains("MODEL=claude-sonnet-5-5"),
        "{env}"
    );
    // Nichts bleibt liegen.
    assert!(!w.state.agent_dir().join("runs").join("BAU-142").exists());
    // Das Ticket zeigt den Branch, aber es gibt keinen erfundenen Pull Request.
    assert_eq!(
        w.ws().item_by_key("BAU-142").unwrap().branch.as_deref(),
        Some(BRANCH)
    );
}

#[test]
fn a_still_not_implemented_tool_starts_no_run() {
    let w = welt(true);
    for a in [Provider::ClaudeCode, Provider::Codex] {
        let state = with_choice(
            &w,
            AgentChoice {
                provider: a,
                endpoint: w.state.agent_choice().endpoint,
            },
        );
        let st = state.agent_status();
        assert!(!st.ready);
        assert!(
            st.missing
                .iter()
                .any(|f| f.contains(a.label()) && f.contains("noch nicht umgesetzt")),
            "{:?}",
            st.missing
        );
        let Err(missing) = state.access() else {
            panic!("kein Zugang für {a:?}");
        };
        assert!(missing[0].contains("noch nicht umgesetzt"));
    }
}

#[test]
fn direct_at_provider_needs_only_key_and_model() {
    let w = welt(true);
    let state = with_choice(
        &w,
        AgentChoice {
            provider: Provider::ClaudeAgentSdk,
            endpoint: Some(EndpointConfig {
                url: None,
                model: Some("claude-opus-5-5".into()),
            }),
        },
    );
    // Der Key liegt unter dem neuen Namen und geht dem älteren vor.
    w.secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-ant-neu"))
        .unwrap();
    let st = state.agent_status();
    assert!(st.ready, "{:?}", st.missing);
    let z = state.access().unwrap();
    assert_eq!(z.endpoint_url, None);
    assert_eq!(z.key.expose(), "sk-ant-neu");
    assert_eq!(z.model, "claude-opus-5-5");

    // Ohne Modell fehlt etwas, und der Hinweis nennt es.
    let state = with_choice(&w, AgentChoice::default());
    let st = state.agent_status();
    assert!(!st.ready);
    assert!(st.missing.iter().any(|f| f.contains("der Modellname")));
}
