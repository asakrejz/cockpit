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
    // The simulated follow-up jobs (own comments, diagnosis) do not exist with the real agent.
    let f = w
        .action(Action::InvestigateCheck {
            review: 85.into(),
            check: "Lint".into(),
        })
        .await
        .unwrap_err();
    assert!(f.contains("mit dem echten Agenten nicht"), "{f}");
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
    // Usage from the run, with the prices from the settings.
    let u = run.usage();
    assert_eq!(
        (u.input_tokens, u.output_tokens, u.millis),
        (1000, 200, 1500)
    );
    assert!(u.cost_cents >= 1);
    assert_eq!(w.ws().spent_month(now_ms()).input_tokens, 1000);
    // Log: from the agent, in plain text.
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
    // The branch is in the remote, main is untouched.
    assert_eq!(w.branches(), [BRANCH, "main"]);
    // Only CLI calls, shared switches, label per run, time limit name, no secret.
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
        "localhost/cockpit-agent:4",
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
    // The value arrives through the environment of the child process.
    let env = w.log("env.log");
    assert!(
        env.contains(&format!("TOKEN={TOKEN}"))
            && env.contains("BASE=https://litellm.example")
            && env.contains("MODEL=claude-sonnet-5-5"),
        "{env}"
    );
    // Nothing is left lying around.
    assert!(!w.state.agent_dir().join("runs").join("BAU-142").exists());
    // The ticket shows the branch, but there is no invented pull request.
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
    // The key is under the new name and goes before the older one.
    w.secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-ant-neu"))
        .unwrap();
    let st = state.agent_status();
    assert!(st.ready, "{:?}", st.missing);
    let z = state.access().unwrap();
    assert_eq!(z.endpoint_url, None);
    assert_eq!(z.key.expose(), "sk-ant-neu");
    assert_eq!(z.model, "claude-opus-5-5");

    // Without a model something is missing, and the hint names it.
    let state = with_choice(&w, AgentChoice::default());
    let st = state.agent_status();
    assert!(!st.ready);
    assert!(st.missing.iter().any(|f| f.contains("der Modellname")));
}

// ---------- Several repos ----------

fn app_and_web() -> Vec<cockpit_core::AgentRepo> {
    vec![
        cockpit_core::AgentRepo::new("firma/app"),
        cockpit_core::AgentRepo {
            // BAU-142 belongs to the project BAU.
            projects: vec!["BAU".into()],
            base_branch: "main".into(),
            ..cockpit_core::AgentRepo::new("firma/web")
        },
    ]
}

#[tokio::test]
async fn a_project_rule_runs_the_ticket_in_its_repo_and_pushes_only_there() {
    let mut w = welt(true);
    w.state = with_repos(&w, app_and_web(), Some("firma/app"));
    w.write("scenario-run.sh", &scenario_ok());
    w.start(false).await;
    assert_eq!(w.run().repo, "firma/web", "the rule BAU → firma/web wins");
    w.wait("Lauf fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    assert_eq!(w.branches_of("web"), [BRANCH, "main"]);
    assert_eq!(w.branches_of("app"), ["main"], "nothing in the other repo");
    assert_eq!(w.run().repo, "firma/web");
}

#[tokio::test]
async fn the_users_choice_goes_before_the_rule() {
    let mut w = welt(true);
    w.state = with_repos(&w, app_and_web(), Some("firma/app"));
    w.write("scenario-run.sh", &scenario_ok());
    w.action(Action::StartAgent {
        ticket: "BAU-142".into(),
        options: AgentOptions {
            plan_first: false,
            repo: Some("firma/app".into()),
            ..AgentOptions::default()
        },
    })
    .await
    .unwrap();
    w.wait("Lauf fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    assert_eq!(w.branches_of("app"), [BRANCH, "main"]);
    assert_eq!(w.branches_of("web"), ["main"]);
}

#[tokio::test]
async fn a_repo_that_is_not_configured_is_rejected_and_nothing_starts() {
    let mut w = welt(true);
    w.state = with_repos(&w, app_and_web(), Some("firma/app"));
    let e = w
        .action(Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions {
                repo: Some("fremd/repo".into()),
                ..AgentOptions::default()
            },
        })
        .await
        .unwrap_err();
    assert!(e.contains("nicht freigegeben"), "{e}");
    assert!(w.run().is_idle());
}

#[tokio::test]
async fn without_rule_and_default_the_user_has_to_choose() {
    let mut w = welt(true);
    // Two repos, none routed for BAU, no default.
    let repos = vec![
        cockpit_core::AgentRepo::new("firma/app"),
        cockpit_core::AgentRepo::new("firma/web"),
    ];
    w.state = with_repos(&w, repos, None);
    let e = w
        .action(Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions::default(),
        })
        .await
        .unwrap_err();
    assert!(e.contains("BAU") && e.contains("Repo"), "{e}");
    assert!(w.run().is_idle());
    // With a choice it starts.
    w.write("scenario-run.sh", &scenario_ok());
    w.action(Action::StartAgent {
        ticket: "BAU-142".into(),
        options: AgentOptions {
            repo: Some("firma/web".into()),
            ..AgentOptions::default()
        },
    })
    .await
    .unwrap();
    assert_eq!(w.run().repo, "firma/web");
}

#[tokio::test]
async fn the_status_reports_repos_default_and_routing() {
    let mut w = welt(true);
    w.state = with_repos(&w, app_and_web(), Some("firma/app"));
    let st = w.state.clone();
    let status = tokio::task::spawn_blocking(move || st.agent_status())
        .await
        .unwrap();
    assert_eq!(status.repo.as_deref(), Some("firma/app"));
    assert_eq!(status.repos.repos.len(), 2);
    assert_eq!(
        status.repos.choose(Some("BAU")).repo(),
        Some("firma/web"),
        "the interface applies the same rules"
    );
}
