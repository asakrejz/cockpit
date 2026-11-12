use super::*;

#[tokio::test]
async fn ohne_einrichtung_startet_kein_lauf_und_die_meldung_sagt_wohin() {
    let w = welt(false);
    std::fs::remove_file(w.bin().join("docker")).unwrap();
    let st = w.state.clone();
    let status = tokio::task::spawn_blocking(move || st.agent_status())
        .await
        .unwrap();
    assert_eq!(status.mode, "echt");
    assert!(!status.ready);
    let text = status.missing.join(" ");
    assert!(
        text.contains("Einrichten")
            && text.contains("Container")
            && text.contains("Modell")
            && text.contains("GitHub"),
        "{text}"
    );
    let fehler = w
        .aktion(Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions::default(),
        })
        .await
        .unwrap_err();
    assert!(
        fehler.contains("Der Agent kann noch nicht starten") && fehler.contains("Einrichten"),
        "{fehler}"
    );
    assert!(w.lauf().is_idle(), "es wurde nichts gestartet");
    // Folgeaufträge gibt es noch nicht mit dem echten Agenten.
    let f = w
        .aktion(Action::InvestigateCheck {
            review: 85,
            check: "Lint".into(),
        })
        .await
        .unwrap_err();
    assert!(f.contains("noch nicht mit dem echten Agenten"), "{f}");
}

#[tokio::test]
async fn fehlendes_image_wird_gemeldet() {
    let w = welt(true);
    std::fs::write(w.bin().join("kein-image"), "").unwrap();
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
async fn ein_lauf_von_der_freigabe_bis_zum_push() {
    let w = welt(true);
    w.schreibe("szenario-lauf.sh", &szenario_ok());
    w.start(false).await;
    w.warte("Lauf fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.warte_auf_ende().await;

    let run = w.lauf();
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
    assert_eq!(w.ws().spent_month(jetzt_ms()).input_tokens, 1000);
    // Protokoll: vom Agenten, in Klartext.
    let agent_eintraege: Vec<String> = w
        .ws()
        .log_entries(Some(Actor::Agent))
        .iter()
        .map(|e| e.text.clone())
        .collect();
    assert!(
        agent_eintraege
            .iter()
            .any(|t| t.contains("Container gestartet")),
        "{agent_eintraege:?}"
    );
    assert!(
        agent_eintraege.iter().any(|t| t.contains("Datei ändern")),
        "{agent_eintraege:?}"
    );
    // Der Branch liegt im Remote, main ist unberührt.
    assert_eq!(w.zweige(), [BRANCH, "main"]);
    // Nur CLI-Aufrufe, gemeinsame Schalter, Label je Lauf, Zeitlimit-Name, kein Geheimnis.
    let aufrufe = w.log("aufrufe.log");
    let lauf_zeile = aufrufe
        .lines()
        .find(|z| z.starts_with("run "))
        .expect("run aufgerufen");
    for muss in [
        "--rm",
        "--label cockpit.lauf=BAU-142",
        "--add-host cockpit-host:host-gateway",
        ":/work ",
        ":/auftrag:ro",
        "-e ANTHROPIC_AUTH_TOKEN",
        "localhost/cockpit-agent:1",
    ] {
        assert!(lauf_zeile.contains(muss), "{muss} fehlt: {lauf_zeile}");
    }
    assert!(
        !aufrufe.contains(TOKEN) && !aufrufe.contains(GITHUB),
        "Geheimnis in der Argumentliste: {aufrufe}"
    );
    assert!(
        !aufrufe.contains("https://litellm.example"),
        "auch die Adresse steht nur in der Umgebung"
    );
    // Der Wert kommt über die Umgebung des Kindprozesses an.
    let env = w.log("umgebung.log");
    assert!(
        env.contains(&format!("TOKEN={TOKEN}"))
            && env.contains("BASE=https://litellm.example")
            && env.contains("MODEL=claude-sonnet-5-5"),
        "{env}"
    );
    // Nichts bleibt liegen.
    assert!(
        !w.state
            .agent_ordner()
            .join("laeufe")
            .join("BAU-142")
            .exists()
    );
    // Das Ticket zeigt den Branch, aber es gibt keinen erfundenen Pull Request.
    assert_eq!(
        w.ws().item_by_key("BAU-142").unwrap().branch.as_deref(),
        Some(BRANCH)
    );
}

#[test]
fn ein_noch_nicht_umgesetztes_werkzeug_startet_keinen_lauf() {
    let w = welt(true);
    for a in [Provider::ClaudeCode, Provider::Codex] {
        let state = mit_wahl(
            &w,
            AgentWahl {
                anbieter: a,
                endpunkt: w.state.agent_wahl().endpunkt,
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
        let Err(fehlt) = state.zugang() else {
            panic!("kein Zugang für {a:?}");
        };
        assert!(fehlt[0].contains("noch nicht umgesetzt"));
    }
}

#[test]
fn direkt_beim_anbieter_braucht_nur_key_und_modell() {
    let w = welt(true);
    let state = mit_wahl(
        &w,
        AgentWahl {
            anbieter: Provider::ClaudeAgentSdk,
            endpunkt: Some(EndpunktKonfig {
                url: None,
                modell: Some("claude-opus-5-5".into()),
            }),
        },
    );
    // Der Key liegt unter dem neuen Namen und geht dem älteren vor.
    w.secrets
        .set(secrets::AGENT_KEY, &Secret::new("sk-ant-neu"))
        .unwrap();
    let st = state.agent_status();
    assert!(st.ready, "{:?}", st.missing);
    let z = state.zugang().unwrap();
    assert_eq!(z.endpunkt_url, None);
    assert_eq!(z.key.expose(), "sk-ant-neu");
    assert_eq!(z.modell, "claude-opus-5-5");

    // Ohne Modell fehlt etwas, und der Hinweis nennt es.
    let state = mit_wahl(&w, AgentWahl::default());
    let st = state.agent_status();
    assert!(!st.ready);
    assert!(st.missing.iter().any(|f| f.contains("der Modellname")));
}
