use super::*;
use cockpit_core::{Action, Actor, AgentOptions, demo};

const T: u64 = 1_700_000_000_000;

#[test]
fn zeilen_werden_gelesen_gekuerzt_und_unbekanntes_verworfen() {
    let e = zeile_lesen(
        r#"{"t":"schritt","titel":"Datei lesen","text":"a","token_ein":10,"token_aus":2,"ms":5}"#,
    )
    .unwrap();
    assert_eq!(
        e,
        Ereignis::Schritt {
            titel: "Datei lesen".into(),
            text: "a".into(),
            token_ein: 10,
            token_aus: 2,
            ms: 5
        }
    );
    assert_eq!(zeile_lesen("das ist kein json"), Err(Verworfen::Unlesbar));
    assert_eq!(zeile_lesen(r#"{"t":"rm -rf"}"#), Err(Verworfen::Unlesbar));
    assert_eq!(zeile_lesen(r#"{"t":"schritt"}"#), Err(Verworfen::Unlesbar));
    assert_eq!(zeile_lesen(""), Err(Verworfen::Unlesbar));
    let gross = format!(r#"{{"t":"fehler","text":"{}"}}"#, "x".repeat(MAX_ZEILE));
    assert_eq!(zeile_lesen(&gross), Err(Verworfen::ZuGross));
    // Übergroße Werte werden begrenzt, nicht übernommen.
    let Ok(Ereignis::Schritt {
        token_ein,
        ms,
        titel,
        ..
    }) = zeile_lesen(&format!(
        r#"{{"t":"schritt","titel":"{}","token_ein":99999999999999,"ms":99999999999}}"#,
        "y".repeat(5_000)
    ))
    else {
        panic!()
    };
    assert_eq!(token_ein, MAX_TOKEN);
    assert_eq!(ms, MAX_MS);
    assert_eq!(titel.chars().count(), 120);
    let Ok(Ereignis::Dateien { dateien }) = zeile_lesen(&format!(
        r#"{{"t":"dateien","dateien":[{}]}}"#,
        vec![r#""a""#; 1_000].join(",")
    )) else {
        panic!()
    };
    assert_eq!(dateien.len(), MAX_LISTE);
}

#[test]
fn ereignisse_werden_nur_im_passenden_modus_zu_meldungen() {
    let plan = Ereignis::Plan {
        summary: "s".into(),
        files: vec![],
        steps: vec![],
    };
    assert!(als_meldung(&plan, Modus::Plan).is_some());
    assert!(als_meldung(&plan, Modus::Umsetzen).is_none());
    let frage = Ereignis::Frage {
        text: "?".into(),
        vorschlaege: vec![],
    };
    assert!(als_meldung(&frage, Modus::Plan).is_none());
    assert!(als_meldung(&frage, Modus::Umsetzen).is_some());
    let fehler = Ereignis::Fehler { text: "x".into() };
    assert!(als_meldung(&fehler, Modus::Umsetzen).is_none());
}

#[test]
fn befehlszeile_ohne_geheimnisse_und_mit_den_gemeinsamen_schaltern() {
    let b = LaufBefehl {
        name: "cockpit-lauf-BAU-142-1",
        lauf: "BAU-142",
        klon: Path::new("/daten/klon"),
        klon_nur_lesen: false,
        auftrag: Path::new("/daten/auftrag"),
        stdin_offen: true,
    };
    let a = run_args(&b);
    let text = a.join(" ");
    for muss in [
        "run --rm -i",
        "--label cockpit.lauf=BAU-142",
        "--add-host cockpit-host:host-gateway",
        "-v /daten/klon:/work -v /daten/auftrag:/auftrag:ro",
        "-e ANTHROPIC_BASE_URL -e ANTHROPIC_AUTH_TOKEN -e ANTHROPIC_MODEL -e ANTHROPIC_API_KEY",
        "--cap-drop ALL",
    ] {
        assert!(text.contains(muss), "{muss} fehlt in {text}");
    }
    assert_eq!(a.last().unwrap(), IMAGE);
    // Voll benanntes Image, kein SELinux-Zusatz, kein --user (steht im Containerfile).
    assert!(IMAGE.starts_with("localhost/"));
    assert!(
        !text.contains(":Z") && !text.contains("--user") && !text.contains("host.docker.internal")
    );
    // Nach `-e` steht nur der Name, nie ein `=` mit Wert.
    for (i, x) in a.iter().enumerate() {
        if x == "-e" {
            assert!(!a[i + 1].contains('='), "{}", a[i + 1]);
        }
    }
    // Die Werte gehören in die Umgebung des Kindprozesses.
    let env = umgebung_fuer_kind(
        Some("http://localhost:4000/"),
        "sk-geheim",
        "claude-sonnet-5-5",
    );
    assert!(env.contains(&(
        "ANTHROPIC_BASE_URL",
        Some("http://cockpit-host:4000".to_string())
    )));
    assert!(env.contains(&("ANTHROPIC_AUTH_TOKEN", Some("sk-geheim".to_string()))));
    // Mit Adresse nie ein `ANTHROPIC_API_KEY` aus der Umgebung der Person.
    assert!(env.contains(&("ANTHROPIC_API_KEY", None)));
    assert!(!text.contains("sk-geheim"));
    let ro = run_args(&LaufBefehl {
        klon_nur_lesen: true,
        stdin_offen: false,
        ..b
    });
    assert!(ro.join(" ").contains("/daten/klon:/work:ro"));
    assert!(!ro.contains(&"-i".to_string()));
}

#[test]
fn direkt_beim_anbieter_gibt_nur_den_api_key_weiter() {
    for leer in [None, Some(""), Some("  ")] {
        let env = umgebung_fuer_kind(leer, "sk-ant-geheim", "claude-opus-5-5");
        assert!(env.contains(&("ANTHROPIC_API_KEY", Some("sk-ant-geheim".to_string()))));
        assert!(env.contains(&("ANTHROPIC_BASE_URL", None)));
        assert!(env.contains(&("ANTHROPIC_AUTH_TOKEN", None)));
        assert!(env.contains(&("ANTHROPIC_MODEL", Some("claude-opus-5-5".to_string()))));
    }
    // Jeder Name, den wir setzen oder entfernen, steht auch in der `-e`-Liste.
    let namen: Vec<&str> = umgebung_fuer_kind(None, "k", "m")
        .iter()
        .map(|(n, _)| *n)
        .collect();
    for n in namen {
        assert!(KIND_UMGEBUNG.contains(&n));
    }
}

#[test]
fn proxy_auf_dem_rechner_heisst_im_container_cockpit_host() {
    for (ein, aus) in [
        ("http://localhost:4000", "http://cockpit-host:4000"),
        ("http://127.0.0.1:4000/", "http://cockpit-host:4000"),
        ("https://localhost/v1", "https://cockpit-host/v1"),
        ("http://[::1]:4000", "http://cockpit-host:4000"),
        ("https://litellm.example", "https://litellm.example"),
        (
            "https://litellm.example:8443/x/",
            "https://litellm.example:8443/x",
        ),
        ("https://localhost.example", "https://localhost.example"),
    ] {
        assert_eq!(fuer_container(ein), aus, "{ein}");
    }
}

#[test]
fn namen_sind_sauber() {
    assert_eq!(container_name("BAU-142", 7), "cockpit-lauf-BAU-142-7");
    assert_eq!(container_name("a b/#1", 7), "cockpit-lauf-a-b--1-7");
    assert_eq!(lauf_ordner("a b/#1"), "a_b__1");
}

fn start(ws: &mut Workspace, plan_first: bool) {
    ws.apply(
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions {
                plan_first,
                ..AgentOptions::default()
            },
        },
        Actor::User,
        T,
    )
    .unwrap();
}

fn info(modus: Modus) -> Info {
    Info {
        modus,
        pausiert: false,
        frage_offen: false,
        hinweise_gesendet: 0,
    }
}

fn laufend(m: Modus) -> HashMap<String, Info> {
    HashMap::from([("BAU-142".to_string(), info(m))])
}

#[test]
fn der_auftrag_traegt_ticket_regeln_und_branch_aber_keine_geheimnisse() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    let run = ws.run("BAU-142").unwrap();
    let j = auftrag_json(&ws, &run, Modus::Umsetzen, "claude/BAU-142-x");
    assert_eq!(j["modus"], "umsetzen");
    assert_eq!(j["ticket"]["key"], "BAU-142");
    assert_eq!(j["branch"], "claude/BAU-142-x");
    assert_eq!(j["basis_branch"], "main");
    let text = j.to_string();
    assert!(!text.contains("TOKEN") && !text.contains("sk-"));
    assert_eq!(pruef_auftrag()["modus"], "pruefung");
}

#[test]
fn aufpasser_startet_plan_und_lauf_je_nach_zustand() {
    let mut ws = demo::workspace();
    assert!(plane(&ws, &HashMap::new(), &[]).is_empty());
    start(&mut ws, true);
    assert_eq!(
        plane(&ws, &HashMap::new(), &[]),
        [Anweisung::Starten("BAU-142".into(), Modus::Plan)]
    );
    // Mit laufendem Plan-Container passiert nichts, bis der Plan da ist.
    assert!(plane(&ws, &laufend(Modus::Plan), &[]).is_empty());
    ws.report_agent(
        "BAU-142",
        AgentReport::Plan {
            summary: "s".into(),
            files: vec![],
            steps: vec![],
        },
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &laufend(Modus::Plan), &[]),
        [Anweisung::Stoppen("BAU-142".into())],
        "ein fertiger Plan-Container wird nicht weiter gebraucht"
    );
    assert!(
        plane(&ws, &HashMap::new(), &[]).is_empty(),
        "wartet auf die Freigabe"
    );
    ws.apply(
        Action::ApprovePlan {
            ticket: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &HashMap::new(), &[]),
        [Anweisung::Starten("BAU-142".into(), Modus::Umsetzen)]
    );
    // Ein gerade gescheiterter Lauf startet nicht sofort neu.
    assert!(plane(&ws, &HashMap::new(), &["BAU-142".to_string()]).is_empty());
}

#[test]
fn aufpasser_stoppt_bei_abbruch_grenze_und_verwaisten_containern() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    assert!(plane(&ws, &laufend(Modus::Umsetzen), &[]).is_empty());
    let mut stopp = ws.clone();
    stopp
        .apply(
            Action::CancelAgent {
                ticket: "BAU-142".into(),
            },
            Actor::User,
            T,
        )
        .unwrap();
    assert_eq!(
        plane(&stopp, &laufend(Modus::Umsetzen), &[]),
        [Anweisung::Stoppen("BAU-142".into())]
    );
    // Grenze erreicht: angehalten.
    let mut grenze = ws.clone();
    let mut l = grenze.limits.clone();
    l.run_budget_cents = Some(1);
    grenze.set_limits(l);
    grenze
        .report_agent(
            "BAU-142",
            AgentReport::Step {
                title: "t".into(),
                text: String::new(),
                input_tokens: 1_000_000,
                output_tokens: 0,
                millis: 1,
            },
            T,
        )
        .unwrap();
    assert_eq!(
        plane(&grenze, &laufend(Modus::Umsetzen), &[]),
        [Anweisung::Stoppen("BAU-142".into())]
    );
    // Container für einen Lauf, den es nicht gibt.
    let leer = demo::workspace();
    assert_eq!(
        plane(&leer, &laufend(Modus::Umsetzen), &[]),
        [Anweisung::Stoppen("BAU-142".into())]
    );
}

#[test]
fn aufpasser_reicht_pause_antwort_und_hinweise_weiter() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    ws.apply(
        Action::PauseAgent {
            run: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &laufend(Modus::Umsetzen), &[]),
        [Anweisung::Pausieren("BAU-142".into())]
    );
    ws.apply(
        Action::ResumeAgent {
            run: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    let mut i = info(Modus::Umsetzen);
    i.pausiert = true;
    let mut m = HashMap::from([("BAU-142".to_string(), i)]);
    assert_eq!(
        plane(&ws, &m, &[]),
        [Anweisung::Fortsetzen("BAU-142".into())]
    );

    // Frage und Antwort.
    ws.report_agent(
        "BAU-142",
        AgentReport::Question {
            text: "Welcher?".into(),
            suggestions: vec![],
        },
        T,
    )
    .unwrap();
    m.get_mut("BAU-142").unwrap().pausiert = false;
    m.get_mut("BAU-142").unwrap().frage_offen = true;
    assert!(plane(&ws, &m, &[]).is_empty(), "solange nicht beantwortet");
    ws.apply(
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "der blaue".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &m, &[]),
        [Anweisung::Antwort("BAU-142".into(), "der blaue".into())]
    );
    m.get_mut("BAU-142").unwrap().frage_offen = false;

    // Hinweise gehen genau einmal raus.
    ws.apply(
        Action::SteerAgent {
            run: "BAU-142".into(),
            text: "Kleine Commits".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &m, &[]),
        [Anweisung::Hinweise(
            "BAU-142".into(),
            vec!["Kleine Commits".into()]
        )]
    );
    m.get_mut("BAU-142").unwrap().hinweise_gesendet = 1;
    assert!(plane(&ws, &m, &[]).is_empty());
}

#[test]
fn ein_noch_nicht_umgesetztes_werkzeug_wird_in_klartext_abgelehnt() {
    let laeuft = ContainerStatus {
        runtime: Some("docker".into()),
        engine_running: true,
        ..ContainerStatus::default()
    };
    for a in [Provider::ClaudeCode, Provider::Codex] {
        let v = Voraussetzungen {
            container: &laeuft,
            image: true,
            anbieter: a,
            modell: true,
            key: true,
            repo: Ok("firma/app"),
            github_zugang: true,
        };
        let f = fehlende(&v);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].contains(a.label()) && f[0].contains("noch nicht umgesetzt"));
        assert!(!status(true, &v).ready);
    }
}

#[test]
fn voraussetzungen_sagen_in_klartext_was_fehlt_und_wohin() {
    let kein = ContainerStatus::default();
    let v = Voraussetzungen {
        container: &kein,
        image: false,
        anbieter: Provider::ClaudeAgentSdk,
        modell: false,
        key: false,
        repo: Err("Es ist kein Repo eingetragen.".into()),
        github_zugang: false,
    };
    let f = fehlende(&v);
    assert_eq!(f.len(), 4);
    assert!(f[0].contains("Einrichten") && f[0].contains("Container"));
    assert!(f[1].contains("der Modellname, der Key") && !f[1].contains("Adresse"));
    assert!(f[3].contains("GitHub"));
    let laeuft = ContainerStatus {
        runtime: Some("docker".into()),
        engine_running: true,
        ..ContainerStatus::default()
    };
    let fast = Voraussetzungen {
        container: &laeuft,
        image: false,
        anbieter: Provider::ClaudeAgentSdk,
        modell: true,
        key: true,
        repo: Ok("firma/app"),
        github_zugang: true,
    };
    let f = fehlende(&fast);
    assert_eq!(f.len(), 1);
    assert!(f[0].contains("Image bauen"));
    let s = status(
        true,
        &Voraussetzungen {
            image: true,
            ..fast
        },
    );
    assert!(s.ready && s.mode == "echt" && s.repo.as_deref() == Some("firma/app"));
    let nicht_laeuft = ContainerStatus {
        runtime: Some("podman".into()),
        engine_running: false,
        ..ContainerStatus::default()
    };
    let f = fehlende(&Voraussetzungen {
        container: &nicht_laeuft,
        image: true,
        anbieter: Provider::ClaudeAgentSdk,
        modell: true,
        key: true,
        repo: Ok("a/b"),
        github_zugang: true,
    });
    assert!(f[0].contains("podman machine start"));
    // Simuliert: immer bereit.
    assert!(status(false, &v).ready);
}

#[test]
fn das_repo_des_agenten() {
    let eins = vec!["a/b".to_string()];
    let zwei = vec!["a/b".to_string(), "c/d".to_string()];
    assert_eq!(repo_waehlen(Some("x/y"), &zwei), Ok("x/y"));
    assert_eq!(repo_waehlen(None, &eins), Ok("a/b"));
    assert!(repo_waehlen(None, &zwei).unwrap_err().contains("[agent]"));
    assert!(repo_waehlen(None, &[]).unwrap_err().contains("kein Repo"));
}
