use super::*;

#[tokio::test]
async fn plan_zuerst_dann_freigabe_dann_lauf() {
    let w = welt(true);
    w.schreibe(
        "szenario-plan.sh",
        r#"printf '%s\n' '{"t":"schritt","titel":"Code durchsuchen","text":"","token_ein":500,"token_aus":50,"ms":800}' '{"t":"plan","summary":"Fokus-Ring ergänzen","files":["neu.txt"],"steps":["Lesen","Ändern"]}'
exit 0
"#,
    );
    w.schreibe("szenario-lauf.sh", &szenario_ok());
    w.start(true).await;
    // Solange der Plan erarbeitet wird, lässt er sich nicht freigeben.
    let f = w
        .aktion(Action::ApprovePlan {
            ticket: "BAU-142".into(),
        })
        .await
        .unwrap_err();
    assert!(f.contains("erarbeitet den Plan noch"), "{f}");
    w.warte("Plan liegt vor", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.agent_plan.is_some())
    })
    .await;
    w.warte_auf_ende().await;
    let run = w.lauf();
    assert!(run.is_awaiting());
    assert_eq!(run.plan().unwrap().summary, "Fokus-Ring ergänzen");
    assert_eq!(run.usage().input_tokens, 500, "auch das Planen kostet");
    assert!(
        w.zweige() == ["main"],
        "vor der Freigabe entsteht nichts im Remote"
    );
    // Der Plan-Container bekommt den Klon nur zum Lesen.
    assert!(
        w.log("aufrufe.log")
            .lines()
            .any(|z| z.starts_with("run ") && z.contains(":/work:ro"))
    );
    // Freigabe: jetzt läuft der Container zum Umsetzen.
    w.aktion(Action::ApprovePlan {
        ticket: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.warte("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.warte_auf_ende().await;
    assert_eq!(w.zweige(), [BRANCH, "main"]);
    assert_eq!(
        w.log("aufrufe.log")
            .lines()
            .filter(|z| z.starts_with("run "))
            .count(),
        2
    );
}

#[tokio::test]
async fn das_budget_stoppt_den_container() {
    let w = welt(true);
    w.schreibe(
        "szenario-lauf.sh",
        r#"printf '%s\n' '{"t":"schritt","titel":"Viel gelesen","text":"","token_ein":1000000,"token_aus":0,"ms":10}'
while [ ! -e "$d/gestoppt-$name" ]; do sleep 0.05; done
exit 137
"#,
    );
    let mut grenzen = w.ws().limits.clone();
    grenzen.run_budget_cents = Some(50);
    w.aktion(Action::SetAgentLimits { limits: grenzen })
        .await
        .unwrap();
    w.start(false).await;
    w.warte("angehalten", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_stopped())
    })
    .await;
    w.warte_auf_ende().await;
    assert!(
        w.log("aufrufe.log")
            .lines()
            .any(|z| z.starts_with("rm -f cockpit-lauf-BAU-142-")),
        "der Container wurde entfernt"
    );
    let run = w.lauf();
    assert!(
        run.messages
            .iter()
            .any(|m| m.text.starts_with("Angehalten: Budget pro Lauf erreicht"))
    );
    assert_eq!(w.zweige(), ["main"]);
    // Der Klon bleibt für das Fortsetzen.
    assert!(
        w.state
            .agent_ordner()
            .join("laeufe/BAU-142/lauf/.git")
            .exists()
    );
    // Sie wird nicht erneut gestartet, solange sie angehalten ist.
    w.state.aufpassen().await;
    assert!(sperre(&w.state.inner.agent.laufend).is_empty());
}

#[tokio::test]
async fn abbruch_durch_den_nutzer_beendet_den_container_und_raeumt_auf() {
    let w = welt(true);
    w.schreibe(
        "szenario-lauf.sh",
        r#"while [ ! -e "$d/gestoppt-$name" ]; do sleep 0.05; done
exit 137
"#,
    );
    w.start(false).await;
    w.warte("Container läuft", |_| {
        !sperre(&w.state.inner.agent.laufend).is_empty() && w.log("aufrufe.log").contains("run ")
    })
    .await;
    w.aktion(Action::CancelAgent {
        ticket: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.warte_auf_ende().await;
    assert!(
        w.log("aufrufe.log")
            .lines()
            .any(|z| z.starts_with("rm -f cockpit-lauf-BAU-142-"))
    );
    let run = w.lauf();
    assert!(run.is_idle());
    assert_eq!(run.ended, None, "ein Abbruch durch dich ist kein Fehler");
    assert!(!w.state.agent_ordner().join("laeufe/BAU-142").exists());
    assert_eq!(w.zweige(), ["main"]);
}

#[tokio::test]
async fn fehler_im_container_stehen_in_klartext_im_lauf_und_ohne_geheimnisse() {
    let w = welt(true);
    // Der Läufer meldet einen Fehler.
    w.schreibe(
        "szenario-lauf.sh",
        r#"printf '%s\n' '{"t":"fehler","text":"Das SDK meldet: Verbindung abgewiesen"}'
exit 1
"#,
    );
    w.start(false).await;
    w.warte("beendet", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    w.warte_auf_ende().await;
    let run = w.lauf();
    assert!(run.is_idle());
    assert_eq!(
        run.ended.as_deref(),
        Some("Das SDK meldet: Verbindung abgewiesen")
    );
    assert!(run.log().contains("Verbindung abgewiesen"));
    assert!(!w.state.agent_ordner().join("laeufe/BAU-142").exists());

    // Der Container stirbt ohne Meldung; seine Fehlerausgabe enthält ein Geheimnis.
    w.schreibe(
        "szenario-lauf.sh",
        &format!("echo \"boom mit {TOKEN} und {GITHUB}\" >&2\nexit 3\n"),
    );
    sperre(&w.state.inner.agent.gescheitert).clear();
    w.start(false).await;
    w.warte("beendet", |ws| {
        ws.run("BAU-142").is_some_and(|r| {
            r.ended
                .as_deref()
                .is_some_and(|e| e.contains("Fehlercode 3"))
        })
    })
    .await;
    let ended = w.lauf().ended.unwrap();
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
async fn docker_startet_nicht_in_klartext() {
    let w = welt(true);
    w.schreibe(
        "szenario-lauf.sh",
        "echo 'Cannot connect to the Docker daemon' >&2\nexit 125\n",
    );
    w.start(false).await;
    w.warte("beendet", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    let e = w.lauf().ended.unwrap();
    assert!(
        e.contains("ließ sich nicht starten") && e.contains("Docker daemon"),
        "{e}"
    );
}

#[tokio::test]
async fn unbrauchbare_zeilen_werden_verworfen_der_rest_gilt() {
    let w = welt(true);
    let mut s = String::from(
        "echo '{{{ kaputt'\necho 'irgendein Text'\necho '{\"t\":\"unbekannt\"}'\necho '{\"t\":\"schritt\"}'\n",
    );
    s.push_str(&format!(
        "head -c 100000 /dev/zero | tr '\\0' x; echo\n{}",
        szenario_ok()
    ));
    w.schreibe("szenario-lauf.sh", &s);
    w.start(false).await;
    w.warte("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    assert_eq!(w.lauf().usage().input_tokens, 1000);
}

#[tokio::test]
async fn rueckfrage_antwort_und_hinweis_gehen_ueber_stdin() {
    let w = welt(true);
    w.schreibe(
        "szenario-lauf.sh",
        &format!(
            r#"printf '%s\n' '{{"t":"frage","text":"Welcher Farbton?","vorschlaege":["blau","rot"]}}'
read -r antwort
echo "$antwort" >> "$d/stdin.log"
read -r hinweis
echo "$hinweis" >> "$d/stdin.log"
printf '%s\n' '{{"t":"hinweise_angekommen"}}'
{}"#,
            szenario_ok()
        ),
    );
    w.start(false).await;
    w.warte("Frage", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_asking())
    })
    .await;
    assert_eq!(w.lauf().question.unwrap().suggestions, ["blau", "rot"]);
    w.aktion(Action::AnswerAgent {
        run: "BAU-142".into(),
        text: "blau".into(),
    })
    .await
    .unwrap();
    w.warte("Antwort angekommen", |_| {
        w.log("stdin.log").contains("antwort")
    })
    .await;
    assert!(
        w.log("stdin.log")
            .contains(r#"{"t":"antwort","text":"blau"}"#),
        "{}",
        w.log("stdin.log")
    );
    w.aktion(Action::SteerAgent {
        run: "BAU-142".into(),
        text: "Kleine Commits".into(),
    })
    .await
    .unwrap();
    w.warte("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    assert!(
        w.log("stdin.log")
            .contains(r#"{"t":"hinweis","text":"Kleine Commits"}"#)
    );
    let run = w.lauf();
    assert!(run.pending_hints.is_empty());
    assert!(
        run.messages
            .iter()
            .any(|m| m.kind == MessageKind::Ack && m.text.contains("Kleine Commits"))
    );
}

#[tokio::test]
async fn pause_und_fortsetzen_gehen_an_den_container() {
    let w = welt(true);
    w.schreibe(
        "szenario-lauf.sh",
        &format!(
            "while [ ! -e \"$d/weiter\" ]; do sleep 0.05; done\n{}",
            szenario_ok()
        ),
    );
    w.start(false).await;
    w.warte("Container", |_| w.log("aufrufe.log").contains("run "))
        .await;
    w.aktion(Action::PauseAgent {
        run: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.warte("pausiert", |_| w.log("pause.log").contains("pause"))
        .await;
    w.aktion(Action::ResumeAgent {
        run: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.warte("fortgesetzt", |_| w.log("pause.log").contains("unpause"))
        .await;
    w.schreibe("weiter", "");
    w.warte("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
}

#[tokio::test]
async fn zeitlimit_beendet_den_container_mit_klartext() {
    let mut w = welt(true);
    // Eigene Einstellungen mit 1 Sekunde Obergrenze.
    let e = Einstellungen {
        lauf_sekunden: Some(1),
        ..w.state.inner.einstellungen.clone()
    };
    let conn = crate::db::open(&w.tmp.path().join("z.db")).unwrap();
    w.state = AppState::neu(
        conn,
        w.secrets.clone(),
        8787,
        crate::abgleich::Quellen::default(),
        e,
    );
    w.state.inner.live.setze_stand_fuer_test(demo::workspace());
    w.schreibe(
        "szenario-lauf.sh",
        "while [ ! -e \"$d/gestoppt-$name\" ]; do sleep 0.05; done\nexit 137\n",
    );
    w.start(false).await;
    w.warte("Zeitlimit", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    let e = w.lauf().ended.unwrap();
    assert!(e.contains("Zeitlimit") && e.contains("beendet"), "{e}");
    assert!(
        w.log("aufrufe.log")
            .lines()
            .any(|z| z.starts_with("rm -f cockpit-lauf-BAU-142-"))
    );
}

#[tokio::test]
async fn verbotene_pfade_verhindern_den_push() {
    let w = welt(true);
    let r = cockpit_core::RepoRules {
        forbidden_paths: vec!["*.txt".into()],
        ..Default::default()
    };
    w.aktion(Action::SetRepoRules {
        repo: cockpit_core::DEMO_REPO.into(),
        rules: r,
    })
    .await
    .unwrap();
    w.schreibe("szenario-lauf.sh", &szenario_ok());
    w.start(false).await;
    w.warte("beendet", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.ended.is_some())
    })
    .await;
    assert!(
        w.lauf()
            .ended
            .unwrap()
            .contains("verstoßen gegen die Regeln")
    );
    assert_eq!(w.zweige(), ["main"]);
}
