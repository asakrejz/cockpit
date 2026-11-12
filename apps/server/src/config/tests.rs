use super::*;

fn keine_env(_: &str) -> Option<String> {
    None
}

#[test]
fn standardwerte() {
    let c = Config::aus(None, keine_env).unwrap();
    assert_eq!(c.port, 8787);
    assert_eq!(c.dist, PathBuf::from("apps/cockpit/dist"));
    assert!(c.db.ends_with("cockpit/cockpit.db"));
    assert!(c.github.is_none() && c.jira.is_none());
}

#[test]
fn datei_ueberschreibt_standard() {
    let toml = r#"
        port = 9000
        dist = "/srv/app"
        [github]
        [jira]
        site = "https://firma.atlassian.net"
        email = "ich@firma.example"
    "#;
    let c = Config::aus(Some(toml), keine_env).unwrap();
    assert_eq!(c.port, 9000);
    assert_eq!(c.dist, PathBuf::from("/srv/app"));
    assert_eq!(c.github.unwrap().api_url, STANDARD_GITHUB_API);
    assert_eq!(c.jira.unwrap().email, "ich@firma.example");
    assert_eq!(c.intervall_s, 120);
}

#[test]
fn quellen_und_intervall() {
    let toml = r#"
        intervall_s = 60
        [github]
        repos = ["firma/app", "firma/web.v2"]
        [jira]
        site = "https://firma.atlassian.net"
        email = "ich@firma.example"
        projects = ["BAU", "WEB"]
        jql = "  "
    "#;
    let c = Config::aus(Some(toml), keine_env).unwrap();
    assert_eq!(c.intervall_s, 60);
    assert_eq!(c.github.unwrap().repos, ["firma/app", "firma/web.v2"]);
    let j = c.jira.unwrap();
    assert_eq!(j.projects, ["BAU", "WEB"]);
    assert_eq!(j.jql, None, "leere Abfrage zählt nicht");
    for kaputt in [
        "intervall_s = 5",
        "[github]\nrepos = [\"nur-name\"]",
        "[github]\nrepos = [\"a/b/c\"]",
        "[github]\nrepos = [\"a b/c\"]",
        "[jira]\nsite = \"https://x\"\nemail = \"a@b\"\nprojects = [\"A) OR (B\"]",
    ] {
        assert!(
            matches!(
                Config::aus(Some(kaputt), keine_env),
                Err(KonfigFehler::Wert(_))
            ),
            "{kaputt}"
        );
    }
}

#[test]
fn umgebung_ueberschreibt_datei() {
    let env = |k: &str| match k {
        "COCKPIT_PORT" => Some("9100".to_string()),
        "COCKPIT_DB" => Some("/tmp/x.db".to_string()),
        _ => None,
    };
    let c = Config::aus(Some("port = 9000"), env).unwrap();
    assert_eq!(c.port, 9100);
    assert_eq!(c.db, PathBuf::from("/tmp/x.db"));
}

#[test]
fn fehler_sind_verstaendlich() {
    let env_kaputt = |k: &str| (k == "COCKPIT_PORT").then(|| "abc".to_string());
    assert!(matches!(
        Config::aus(None, env_kaputt),
        Err(KonfigFehler::Wert(_))
    ));
    assert!(matches!(
        Config::aus(Some("port = "), keine_env),
        Err(KonfigFehler::Toml(_))
    ));
    // Tippfehler in der Datei fallen auf, statt still ignoriert zu werden.
    assert!(matches!(
        Config::aus(Some("prot = 1"), keine_env),
        Err(KonfigFehler::Toml(_))
    ));
    assert!(matches!(
        Config::aus(Some("port = 0"), keine_env),
        Err(KonfigFehler::Wert(_))
    ));
    // Jira braucht Adresse und E-Mail.
    assert!(Config::aus(Some("[jira]\nsite = \"https://x\""), keine_env).is_err());
}

#[test]
fn oauth_abschnitte() {
    let toml = r#"
        [github_oauth]
        client_id = " Iv23abc "
        [atlassian]
        client_id = "atl123"
        site = "firma"
    "#;
    let c = Config::aus(Some(toml), keine_env).unwrap();
    assert_eq!(c.github_oauth.unwrap().client_id, "Iv23abc");
    let a = c.atlassian.unwrap();
    assert_eq!(
        (a.client_id.as_str(), a.site.as_deref()),
        ("atl123", Some("firma"))
    );
    // [atlassian] allein richtet Jira ein, ohne site und email.
    let j = c.jira.unwrap();
    assert!(j.site.is_empty() && j.email.is_empty() && j.projects.is_empty());

    // Projekte bleiben einstellbar.
    let toml = "[atlassian]\nclient_id = \"a\"\n[jira]\nprojects = [\"BAU\"]";
    assert_eq!(
        Config::aus(Some(toml), keine_env)
            .unwrap()
            .jira
            .unwrap()
            .projects,
        ["BAU"]
    );
    for kaputt in [
        "[github_oauth]\nclient_id = \"\"",
        "[atlassian]\nclient_id = \"a b\"",
    ] {
        assert!(matches!(
            Config::aus(Some(kaputt), keine_env),
            Err(KonfigFehler::Wert(_))
        ));
    }
    // Tippfehler im Abschnitt fallen auf (Secrets gehören nicht in die Datei).
    assert!(matches!(
        Config::aus(
            Some("[atlassian]\nclient_id = \"a\"\nclient_secret = \"x\""),
            keine_env
        ),
        Err(KonfigFehler::Toml(_))
    ));
}

#[test]
fn seed_ist_standardmaessig_leer() {
    assert_eq!(Config::aus(None, keine_env).unwrap().seed, Seed::Leer);
    assert_eq!(
        Config::aus(Some("seed = \"demo\""), keine_env)
            .unwrap()
            .seed,
        Seed::Demo
    );
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| "leer".to_string());
    assert_eq!(
        Config::aus(Some("seed = \"demo\""), env).unwrap().seed,
        Seed::Leer,
        "die Umgebung geht der Datei vor"
    );
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| " Demo ".to_string());
    assert_eq!(Config::aus(None, env).unwrap().seed, Seed::Demo);
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| "viel".to_string());
    assert!(matches!(Config::aus(None, env), Err(KonfigFehler::Wert(_))));
    assert!(Config::aus(Some("seed = \"viel\""), keine_env).is_err());
}

#[test]
fn repo_und_ordner_des_agenten() {
    let toml = "[agent]\nrepo = \" firma/app \"\ndir = \"/opt/cockpit/agent\"";
    let c = Config::aus(Some(toml), keine_env).unwrap();
    assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
    assert_eq!(c.agent_dir, Some(PathBuf::from("/opt/cockpit/agent")));
    let env = |k: &str| match k {
        "COCKPIT_AGENT_DIR" => Some("/andere/agent".to_string()),
        "COCKPIT_GIT_BASIS" => Some("file:///tmp/git/".to_string()),
        _ => None,
    };
    let c = Config::aus(Some(toml), env).unwrap();
    assert_eq!(c.agent_dir, Some(PathBuf::from("/andere/agent")));
    assert_eq!(c.git_basis.as_deref(), Some("file:///tmp/git"));
    for kaputt in ["repo = \"firma\"", "repo = \"../x\"", "repo = \"a/b/c\""] {
        assert!(
            matches!(
                Config::aus(Some(&format!("[agent]\n{kaputt}")), keine_env),
                Err(KonfigFehler::Wert(_))
            ),
            "{kaputt}"
        );
    }
    // Ohne Angabe nichts.
    let c = Config::aus(None, keine_env).unwrap();
    assert_eq!((c.agent_repo, c.agent_dir, c.git_basis), (None, None, None));
}

#[test]
fn endpunkt_und_container() {
    let toml = r#"
        [agent]
        container = "Podman"
        anbieter = "Claude-Agent-SDK"
        [agent.endpunkt]
        url = " http://localhost:4000/ "
        modell = "claude-sonnet-5-5"
    "#;
    let c = Config::aus(Some(toml), keine_env).unwrap();
    assert_eq!(c.anbieter, Provider::ClaudeAgentSdk);
    let e = c.endpunkt.unwrap();
    assert_eq!(e.url.as_deref(), Some("http://localhost:4000"));
    assert_eq!(e.modell.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(c.container.as_deref(), Some("podman"));
    assert_eq!(c.container_env, None);

    let env = |k: &str| (k == "COCKPIT_CONTAINER").then(|| "docker".to_string());
    assert_eq!(
        Config::aus(None, env).unwrap().container_env.as_deref(),
        Some("docker")
    );
    for kaputt in [
        "[agent.endpunkt]\nurl = \"localhost:4000\"",
        "[agent.endpunkt]\nurl = \"http://\"",
        "[agent.endpunkt]\nurl = \"http://a b\"",
        "[agent]\ncontainer = \"nerdctl\"",
        "[agent]\nanbieter = \"gpt\"",
        "[litellm]\nurl = \"localhost:4000\"",
    ] {
        assert!(
            matches!(
                Config::aus(Some(kaputt), keine_env),
                Err(KonfigFehler::Wert(_))
            ),
            "{kaputt}"
        );
    }
    // Der Key gehört nicht in die Datei.
    for mit_key in [
        "[agent.endpunkt]\nurl = \"http://x\"\nkey = \"abc\"",
        "[litellm]\nurl = \"http://x\"\nkey = \"abc\"",
    ] {
        assert!(matches!(
            Config::aus(Some(mit_key), keine_env),
            Err(KonfigFehler::Toml(_))
        ));
    }
}

#[test]
fn direkt_beim_anbieter_hat_keine_adresse() {
    let c = Config::aus(
        Some("[agent.endpunkt]\nmodell = \"claude-opus-5-5\""),
        keine_env,
    )
    .unwrap();
    let e = c.endpunkt.unwrap();
    assert_eq!(
        (e.url, e.modell.as_deref()),
        (None, Some("claude-opus-5-5"))
    );
    // Leere Adresse = direkt; ganz leerer Abschnitt = nicht eingerichtet.
    let c = Config::aus(
        Some("[agent.endpunkt]\nurl = \"\"\nmodell = \"m\""),
        keine_env,
    )
    .unwrap();
    assert_eq!(c.endpunkt.unwrap().url, None);
    assert_eq!(
        Config::aus(Some("[agent.endpunkt]"), keine_env)
            .unwrap()
            .endpunkt,
        None
    );
    assert_eq!(
        Config::aus(None, keine_env).unwrap().anbieter,
        Provider::ClaudeAgentSdk
    );
}

#[test]
fn bekannte_werkzeuge_lassen_sich_eintragen() {
    for (text, soll) in [
        ("claude-code", Provider::ClaudeCode),
        ("codex", Provider::Codex),
    ] {
        let c = Config::aus(Some(&format!("[agent]\nanbieter = \"{text}\"")), keine_env).unwrap();
        assert_eq!(c.anbieter, soll);
    }
}

#[test]
fn alte_litellm_datei_wird_weiter_gelesen_und_die_neue_geht_vor() {
    let alt = "[litellm]\nurl = \"http://localhost:4000/\"\nmodell = \"altes-modell\"\n[agent]\ncontainer = \"docker\"";
    let c = Config::aus(Some(alt), keine_env).unwrap();
    let e = c.endpunkt.unwrap();
    assert_eq!(e.url.as_deref(), Some("http://localhost:4000"));
    assert_eq!(e.modell.as_deref(), Some("altes-modell"));
    assert_eq!(c.anbieter, Provider::ClaudeAgentSdk);
    assert_eq!(c.container.as_deref(), Some("docker"));

    let neu = "[agent.endpunkt]\nurl = \"https://litellm.example\"\nmodell = \"neues-modell\"";
    let c = Config::aus(Some(neu), keine_env).unwrap();
    assert_eq!(
        c.endpunkt,
        Config::aus(Some(&format!("{alt}\n{neu}")), keine_env)
            .unwrap()
            .endpunkt
    );
    let e = c.endpunkt.unwrap();
    assert_eq!(e.url.as_deref(), Some("https://litellm.example"));
    assert_eq!(e.modell.as_deref(), Some("neues-modell"));
}

#[test]
fn dev_port_nur_aus_der_umgebung() {
    assert_eq!(Config::aus(None, keine_env).unwrap().dev_port, None);
    let env = |k: &str| (k == "COCKPIT_DEV_PORT").then(|| "8081".to_string());
    assert_eq!(Config::aus(None, env).unwrap().dev_port, Some(8081));
    let env = |k: &str| (k == "COCKPIT_DEV_PORT").then(|| "x".to_string());
    assert!(Config::aus(None, env).is_err());
    assert!(Config::aus(Some("dev_port = 8081"), keine_env).is_err());
}
