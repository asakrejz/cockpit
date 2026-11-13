use super::*;

#[tokio::test]
async fn plan_first_then_approval_then_run() {
    let w = welt(true);
    w.write(
        "scenario-plan.sh",
        r#"printf '%s\n' '{"t":"step","title":"Code durchsuchen","text":"","tokens_in":500,"tokens_out":50,"ms":800}' '{"t":"plan","summary":"Fokus-Ring ergänzen","files":["neu.txt"],"steps":["Lesen","Ändern"]}'
exit 0
"#,
    );
    w.write("scenario-run.sh", &scenario_ok());
    w.start(true).await;
    // Solange der Plan erarbeitet wird, lässt er sich nicht freigeben.
    let f = w
        .action(Action::ApprovePlan {
            ticket: "BAU-142".into(),
        })
        .await
        .unwrap_err();
    assert!(f.contains("erarbeitet den Plan noch"), "{f}");
    w.wait("Plan liegt vor", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.agent_plan.is_some())
    })
    .await;
    w.wait_for_end().await;
    let run = w.run();
    assert!(run.is_awaiting());
    assert_eq!(run.plan().unwrap().summary, "Fokus-Ring ergänzen");
    assert_eq!(run.usage().input_tokens, 500, "auch das Planen kostet");
    assert!(
        w.branches() == ["main"],
        "vor der Freigabe entsteht nichts im Remote"
    );
    // Der Plan-Container bekommt den Klon nur zum Lesen.
    assert!(
        w.log("calls.log")
            .lines()
            .any(|z| z.starts_with("run ") && z.contains(":/work:ro"))
    );
    // Freigabe: jetzt läuft der Container zum Umsetzen.
    w.action(Action::ApprovePlan {
        ticket: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    assert_eq!(w.branches(), [BRANCH, "main"]);
    assert_eq!(
        w.log("calls.log")
            .lines()
            .filter(|z| z.starts_with("run "))
            .count(),
        2
    );
}

#[tokio::test]
async fn the_budget_stops_the_container() {
    let w = welt(true);
    w.write(
        "scenario-run.sh",
        r#"printf '%s\n' '{"t":"step","title":"Viel gelesen","text":"","tokens_in":1000000,"tokens_out":0,"ms":10}'
while [ ! -e "$d/stopped-$name" ]; do sleep 0.05; done
exit 137
"#,
    );
    let mut limits = w.ws().limits.clone();
    limits.run_budget_cents = Some(50);
    w.action(Action::SetAgentLimits { limits }).await.unwrap();
    w.start(false).await;
    w.wait("angehalten", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_stopped())
    })
    .await;
    w.wait_for_end().await;
    assert!(
        w.log("calls.log")
            .lines()
            .any(|z| z.starts_with("rm -f cockpit-run-BAU-142-")),
        "der Container wurde entfernt"
    );
    let run = w.run();
    assert!(
        run.messages
            .iter()
            .any(|m| m.text.starts_with("Angehalten: Budget pro Lauf erreicht"))
    );
    assert_eq!(w.branches(), ["main"]);
    // Der Klon bleibt für das Fortsetzen.
    assert!(w.state.agent_dir().join("runs/BAU-142/run/.git").exists());
    // Sie wird nicht erneut gestartet, solange sie angehalten ist.
    w.state.watch().await;
    assert!(lock(&w.state.inner.agent.running).is_empty());
}

#[tokio::test]
async fn cancel_through_the_user_ended_the_container_and_cleans_on() {
    let w = welt(true);
    w.write(
        "scenario-run.sh",
        r#"while [ ! -e "$d/stopped-$name" ]; do sleep 0.05; done
exit 137
"#,
    );
    w.start(false).await;
    w.wait("Container läuft", |_| {
        !lock(&w.state.inner.agent.running).is_empty() && w.log("calls.log").contains("run ")
    })
    .await;
    w.action(Action::CancelAgent {
        ticket: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.wait_for_end().await;
    assert!(
        w.log("calls.log")
            .lines()
            .any(|z| z.starts_with("rm -f cockpit-run-BAU-142-"))
    );
    let run = w.run();
    assert!(run.is_idle());
    assert_eq!(run.ended, None, "ein Abbruch durch dich ist kein Fehler");
    assert!(!w.state.agent_dir().join("runs/BAU-142").exists());
    assert_eq!(w.branches(), ["main"]);
}

#[tokio::test]
async fn error_in_container_state_in_plain_text_in_run_and_without_secrets() {
    let w = welt(true);
    // Der Läufer meldet einen Fehler.
    w.write(
        "scenario-run.sh",
        r#"printf '%s\n' '{"t":"error","text":"Das SDK meldet: Verbindung abgewiesen"}'
exit 1
"#,
    );
    w.start(false).await;
    w.wait("beendet", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    w.wait_for_end().await;
    let run = w.run();
    assert!(run.is_idle());
    assert_eq!(
        run.ended.as_deref(),
        Some("Das SDK meldet: Verbindung abgewiesen")
    );
    assert!(run.log().contains("Verbindung abgewiesen"));
    assert!(!w.state.agent_dir().join("runs/BAU-142").exists());

    // Der Container stirbt ohne Meldung; seine Fehlerausgabe enthält ein Geheimnis.
    w.write(
        "scenario-run.sh",
        &format!("echo \"boom mit {TOKEN} und {GITHUB}\" >&2\nexit 3\n"),
    );
    lock(&w.state.inner.agent.failed).clear();
    w.start(false).await;
    w.wait("beendet", |ws| {
        ws.run("BAU-142").is_some_and(|r| {
            r.ended
                .as_deref()
                .is_some_and(|e| e.contains("Fehlercode 3"))
        })
    })
    .await;
    let ended = w.run().ended.unwrap();
    assert!(ended.contains("boom mit"), "{ended}");
    assert!(!ended.contains(TOKEN) && !ended.contains(GITHUB), "{ended}");
    assert!(
        !w.ws()
            .log
            .iter()
            .any(|e| e.text.contains(TOKEN) || e.text.contains(GITHUB))
    );
}

#[tokio::test]
async fn docker_starts_not_in_plain_text() {
    let w = welt(true);
    w.write(
        "scenario-run.sh",
        "echo 'Cannot connect to the Docker daemon' >&2\nexit 125\n",
    );
    w.start(false).await;
    w.wait("beendet", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    let e = w.run().ended.unwrap();
    assert!(
        e.contains("ließ sich nicht starten") && e.contains("Docker daemon"),
        "{e}"
    );
}

#[tokio::test]
async fn unusable_lines_are_discarded_the_rest_applies() {
    let w = welt(true);
    let mut s = String::from(
        "echo '{{{ kaputt'\necho 'irgendein Text'\necho '{\"t\":\"unbekannt\"}'\necho '{\"t\":\"step\"}'\n",
    );
    s.push_str(&format!(
        "head -c 100000 /dev/zero | tr '\\0' x; echo\n{}",
        scenario_ok()
    ));
    w.write("scenario-run.sh", &s);
    w.start(false).await;
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    assert_eq!(w.run().usage().input_tokens, 1000);
}

#[tokio::test]
async fn ask_user_answer_and_hint_go_via_stdin() {
    let w = welt(true);
    w.write(
        "scenario-run.sh",
        &format!(
            r#"printf '%s\n' '{{"t":"question","text":"Welcher Farbton?","suggestions":["blau","rot"]}}'
read -r antwort
echo "$antwort" >> "$d/stdin.log"
read -r hinweis
echo "$hinweis" >> "$d/stdin.log"
printf '%s\n' '{{"t":"hints_arrived"}}'
{}"#,
            scenario_ok()
        ),
    );
    w.start(false).await;
    w.wait("Frage", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_asking())
    })
    .await;
    assert_eq!(w.run().question.unwrap().suggestions, ["blau", "rot"]);
    w.action(Action::AnswerAgent {
        run: "BAU-142".into(),
        text: "blau".into(),
    })
    .await
    .unwrap();
    w.wait("Antwort angekommen", |_| {
        w.log("stdin.log").contains("answer")
    })
    .await;
    assert!(
        w.log("stdin.log")
            .contains(r#"{"t":"answer","text":"blau"}"#),
        "{}",
        w.log("stdin.log")
    );
    w.action(Action::SteerAgent {
        run: "BAU-142".into(),
        text: "Kleine Commits".into(),
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    assert!(
        w.log("stdin.log")
            .contains(r#"{"t":"hint","text":"Kleine Commits"}"#)
    );
    let run = w.run();
    assert!(run.pending_hints.is_empty());
    assert!(
        run.messages
            .iter()
            .any(|m| m.kind == MessageKind::Ack && m.text.contains("Kleine Commits"))
    );
}

#[tokio::test]
async fn pause_and_resume_go_at_the_container() {
    let w = welt(true);
    w.write(
        "scenario-run.sh",
        &format!(
            "while [ ! -e \"$d/weiter\" ]; do sleep 0.05; done\n{}",
            scenario_ok()
        ),
    );
    w.start(false).await;
    w.wait("Container", |_| w.log("calls.log").contains("run "))
        .await;
    w.action(Action::PauseAgent {
        run: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.wait("pausiert", |_| w.log("pause.log").contains("pause"))
        .await;
    w.action(Action::ResumeAgent {
        run: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.wait("fortgesetzt", |_| w.log("pause.log").contains("unpause"))
        .await;
    w.write("weiter", "");
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
}

#[tokio::test]
async fn timeout_ended_the_container_with_plain_text() {
    let mut w = welt(true);
    // Eigene Einstellungen mit 1 Sekunde Obergrenze.
    let e = Settings {
        run_seconds: Some(1),
        ..w.state.inner.settings.clone()
    };
    let conn = crate::db::open(&w.tmp.path().join("z.db")).unwrap();
    w.state = AppState::create(
        conn,
        w.secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        e,
    );
    w.state.inner.live.set_state_for_test(demo::workspace());
    w.write(
        "scenario-run.sh",
        "while [ ! -e \"$d/stopped-$name\" ]; do sleep 0.05; done\nexit 137\n",
    );
    w.start(false).await;
    w.wait("Zeitlimit", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    let e = w.run().ended.unwrap();
    assert!(e.contains("Zeitlimit") && e.contains("beendet"), "{e}");
    assert!(
        w.log("calls.log")
            .lines()
            .any(|z| z.starts_with("rm -f cockpit-run-BAU-142-"))
    );
}

#[tokio::test]
async fn forbidden_paths_prevent_the_push() {
    let w = welt(true);
    let r = cockpit_core::RepoRules {
        forbidden_paths: vec!["*.txt".into()],
        ..Default::default()
    };
    w.action(Action::SetRepoRules {
        repo: cockpit_core::DEMO_REPO.into(),
        rules: r,
    })
    .await
    .unwrap();
    w.write("scenario-run.sh", &scenario_ok());
    w.start(false).await;
    w.wait("beendet", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    assert!(
        w.run()
            .ended
            .unwrap()
            .contains("verstoßen gegen die Regeln")
    );
    assert_eq!(w.branches(), ["main"]);
}
