use super::*;
use cockpit_i18n::Lang;

#[tokio::test]
async fn without_setup_starts_no_run_and_the_message_says_where() {
    let w = welt(false);
    std::fs::remove_file(w.bin().join("docker")).unwrap();
    let st = w.state.clone();
    let status = tokio::task::spawn_blocking(move || st.agent_status())
        .await
        .unwrap();
    assert!(!status.ready);
    let text = status
        .missing
        .iter()
        .map(|m| m.render(Lang::De))
        .collect::<Vec<_>>()
        .join(" ");
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
    assert!(w.run().is_idle(), "nothing was started");
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
        status.missing[0].render(Lang::De).contains("Image bauen"),
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
        german(&run.summary).contains("Branch claude/BAU-142")
            && german(&run.summary).contains("gepusht"),
        "{}",
        german(&run.summary)
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
        .map(|e| e.text_in(Lang::De))
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
        .expect("run was called");
    for muss in [
        "--rm",
        "--label cockpit.run=BAU-142",
        "--add-host cockpit-host:host-gateway",
        ":/work ",
        ":/job:ro",
        "-e ANTHROPIC_AUTH_TOKEN",
        "localhost/cockpit-agent:local",
    ] {
        assert!(run_line.contains(muss), "{muss} is missing: {run_line}");
    }
    assert!(
        !calls.contains(TOKEN) && !calls.contains(GITHUB),
        "Geheimnis in der Argumentliste: {calls}"
    );
    assert!(
        !calls.contains("https://litellm.example"),
        "the address is only in the environment too"
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

#[tokio::test]
async fn claude_code_and_codex_start_runs_with_their_adapter_and_names() {
    for (provider, names, values) in [
        (
            Provider::ClaudeCode,
            "-e COCKPIT_PROVIDER -e ANTHROPIC_BASE_URL -e ANTHROPIC_AUTH_TOKEN -e ANTHROPIC_MODEL -e ANTHROPIC_API_KEY",
            format!(
                "TOKEN={TOKEN} BASE=https://litellm.example MODEL=claude-sonnet-5-5 PROVIDER=claude-code OAIKEY= OAIBASE= CMODEL= ANTKEY="
            ),
        ),
        (
            Provider::Codex,
            "-e COCKPIT_PROVIDER -e OPENAI_BASE_URL -e OPENAI_API_KEY -e COCKPIT_MODEL",
            format!(
                "TOKEN= BASE= MODEL= PROVIDER=codex OAIKEY={TOKEN} OAIBASE=https://litellm.example CMODEL=claude-sonnet-5-5 ANTKEY="
            ),
        ),
    ] {
        let w = welt_with(true, provider);
        assert!(w.state.agent_status().ready, "{provider:?}");
        w.write("scenario-run.sh", &scenario_ok());
        w.start(false).await;
        w.wait("Lauf fertig", |ws| {
            ws.run("BAU-142").is_some_and(|r| r.is_done())
        })
        .await;
        w.wait_for_end().await;
        let calls = w.log("calls.log");
        let run_line = calls.lines().find(|z| z.starts_with("run ")).unwrap();
        assert!(run_line.contains(names), "{provider:?}: {run_line}");
        assert!(!calls.contains(TOKEN), "{provider:?}: secret in arguments");
        // The values arrive only through the environment, the other tool family is empty.
        let env = w.log("env.log");
        assert!(env.contains(&values), "{provider:?}: {env}");
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
    assert!(
        st.missing
            .iter()
            .any(|f| f.render(Lang::De).contains("der Modellname"))
    );
}

// ---------- Several repos ----------

fn app_and_web() -> Vec<cockpit_core::AgentRepo> {
    vec![
        cockpit_core::AgentRepo::new("firma/app"),
        cockpit_core::AgentRepo::new("firma/web"),
    ]
}

#[tokio::test]
async fn a_start_without_a_choice_runs_in_the_default_repo_and_pushes_only_there() {
    let mut w = welt(true);
    w.state = with_repos(&w, app_and_web(), Some("firma/app"));
    w.write("scenario-run.sh", &scenario_ok());
    w.start(false).await;
    assert_eq!(w.run().repo, "firma/app", "the project BAU plays no part");
    w.wait("Lauf fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    assert_eq!(w.branches_of("app"), [BRANCH, "main"]);
    assert_eq!(w.branches_of("web"), ["main"], "nothing in the other repo");
    assert_eq!(w.run().repo, "firma/app");
}

#[tokio::test]
async fn the_users_choice_goes_before_the_default() {
    let mut w = welt(true);
    w.state = with_repos(&w, app_and_web(), Some("firma/app"));
    w.write("scenario-run.sh", &scenario_ok());
    w.action(Action::StartAgent {
        ticket: "BAU-142".into(),
        options: AgentOptions {
            plan_first: false,
            repo: Some("firma/web".into()),
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
    assert_eq!(w.branches_of("web"), [BRANCH, "main"]);
    assert_eq!(w.branches_of("app"), ["main"]);
}

#[tokio::test]
async fn claude_code_and_codex_run_in_the_default_and_the_chosen_repo() {
    for provider in [Provider::ClaudeCode, Provider::Codex] {
        // (user choice, repo where the branch must land, repo that stays untouched)
        for (choice, hit, other) in [(None, "app", "web"), (Some("firma/web"), "web", "app")] {
            let mut w = welt_with(true, provider);
            w.state = with_repos(&w, app_and_web(), Some("firma/app"));
            w.write("scenario-run.sh", &scenario_ok());
            w.action(Action::StartAgent {
                ticket: "BAU-142".into(),
                options: AgentOptions {
                    plan_first: false,
                    repo: choice.map(str::to_string),
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
            assert_eq!(
                w.branches_of(hit),
                [BRANCH, "main"],
                "{provider:?} {choice:?}"
            );
            assert_eq!(w.branches_of(other), ["main"], "{provider:?} {choice:?}");
            assert_eq!(w.run().repo, format!("firma/{hit}"));
            let env = w.log("env.log");
            assert!(
                env.contains(&format!("PROVIDER={}", provider.id())),
                "{env}"
            );
            let calls = w.log("calls.log");
            assert!(!calls.contains(TOKEN) && calls.contains("localhost/cockpit-agent:local"));
        }
    }
}

#[tokio::test]
async fn a_repo_name_that_is_no_owner_slash_name_is_rejected_and_nothing_starts() {
    let mut w = welt(true);
    w.state = with_repos(&w, app_and_web(), Some("firma/app"));
    let e = w
        .action(Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions {
                repo: Some("not a repo".into()),
                ..AgentOptions::default()
            },
        })
        .await
        .unwrap_err();
    assert!(e.contains("kein gültiger Repo-Name"), "{e}");
    assert!(w.run().is_idle());
}

#[tokio::test]
async fn without_a_default_the_user_has_to_choose() {
    let mut w = welt(true);
    // Two repos and no default.
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
    assert!(e.contains("Standard-Repo") && e.contains("Repo"), "{e}");
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

#[test]
fn the_only_configured_repo_is_the_default_of_a_start() {
    let w = welt(true);
    let state = with_repos(&w, vec![cockpit_core::AgentRepo::new("firma/app")], None);
    let routed = state
        .agent_route(Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions::default(),
        })
        .unwrap();
    let Action::StartAgent { options, .. } = routed else {
        panic!("a start stays a start");
    };
    assert_eq!(options.repo.as_deref(), Some("firma/app"));
    assert_eq!(options.chosen_repos(), ["firma/app"]);
}

#[tokio::test]
async fn the_status_reports_repos_and_default() {
    let mut w = welt(true);
    w.state = with_repos(&w, app_and_web(), Some("firma/app"));
    let st = w.state.clone();
    let status = tokio::task::spawn_blocking(move || st.agent_status())
        .await
        .unwrap();
    assert_eq!(status.repo.as_deref(), Some("firma/app"));
    assert_eq!(status.repos.repos.len(), 2);
    assert_eq!(
        status.repos.default.as_deref(),
        Some("firma/app"),
        "the interface preselects the same repo"
    );
}

async fn image_state_of(w: &World) -> (cockpit_core::api::ImageState, AgentStatus) {
    let st = w.state.clone();
    let status = tokio::task::spawn_blocking(move || st.agent_status())
        .await
        .unwrap();
    (status.image_state, status)
}

#[tokio::test]
async fn image_states_missing_outdated_current_unknown() {
    use cockpit_core::api::ImageState;
    // Current: the label equals the fingerprint of the sources.
    let w = welt(true);
    let (s, status) = image_state_of(&w).await;
    assert_eq!(s, ImageState::Current);
    assert!(status.ready && status.image, "{:?}", status.missing);
    let calls = w.log("calls.log");
    assert!(
        calls.contains("image inspect --format {{ index .Config.Labels \"cockpit.fingerprint\" }} localhost/cockpit-agent:local"),
        "{calls}"
    );

    // Label differs.
    let w = welt(true);
    w.write("label.out", "0000\n");
    let (s, status) = image_state_of(&w).await;
    assert_eq!(s, ImageState::Outdated);
    assert!(!status.ready && status.image);
    assert!(
        status.missing[0].render(Lang::De).contains("veraltet"),
        "{:?}",
        status.missing
    );

    // Label missing: empty output, or the placeholder some versions print.
    for out in ["\n", "<no value>\n"] {
        let w = welt(true);
        w.write("label.out", out);
        assert_eq!(image_state_of(&w).await.0, ImageState::Outdated, "{out:?}");
    }

    // No image.
    let w = welt(true);
    w.write("no-image", "");
    let (s, status) = image_state_of(&w).await;
    assert_eq!(s, ImageState::Missing);
    assert!(!status.image);

    // The inspect call itself fails (not "no such image"): treated like no image, no crash.
    let w = welt(true);
    w.write("inspect-error", "");
    assert_eq!(image_state_of(&w).await.0, ImageState::Missing);

    // The sources are not there: not checkable, with the reason in plain text.
    let w = welt(true);
    std::fs::remove_dir_all(
        w.state
            .inner
            .settings
            .agent_dir
            .clone()
            .unwrap()
            .join("runner"),
    )
    .unwrap();
    let (s, status) = image_state_of(&w).await;
    assert_eq!(s, ImageState::Unknown);
    assert!(!status.ready && status.image);
    assert!(
        status
            .image_hint
            .as_ref()
            .is_some_and(|h| h.render(Lang::De).contains("runner")),
        "{status:?}"
    );
    assert!(
        status.missing[0].render(Lang::De).contains("nicht prüfen"),
        "{:?}",
        status.missing
    );
}

#[tokio::test]
async fn start_refused_with_outdated_image_and_allowed_with_current() {
    let w = welt(true);
    w.write("label.out", "stale\n");
    let e = w
        .action(Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions::default(),
        })
        .await
        .unwrap_err();
    assert!(e.contains("veraltet") && e.contains("neu bauen"), "{e}");
    assert!(
        !w.log("calls.log").contains("run "),
        "no run with the old image"
    );
    // After the image is current again (new answer after the short cache time) the start works.
    let w = welt(true);
    w.start(false).await;
    w.wait("Container", |_| w.log("calls.log").contains("run "))
        .await;
}
