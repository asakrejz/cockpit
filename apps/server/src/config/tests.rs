use super::*;

fn no_env(_: &str) -> Option<String> {
    None
}

#[test]
fn defaults() {
    let c = Config::parse(None, no_env).unwrap();
    assert_eq!(c.port, 8787);
    assert_eq!(c.dist, PathBuf::from("apps/cockpit/dist"));
    assert!(c.db.ends_with("cockpit/cockpit.db"));
    assert!(c.github.is_none() && c.jira.is_none());
}

#[test]
fn file_overwrites_defaults() {
    let toml = r#"
        port = 9000
        dist = "/srv/app"
        [github]
        [jira]
        site = "https://firma.atlassian.net"
        email = "ich@firma.example"
    "#;
    let c = Config::parse(Some(toml), no_env).unwrap();
    assert_eq!(c.port, 9000);
    assert_eq!(c.dist, PathBuf::from("/srv/app"));
    assert_eq!(c.github.unwrap().api_url, DEFAULT_GITHUB_API);
    assert_eq!(c.jira.unwrap().email, "ich@firma.example");
    assert_eq!(c.interval_s, 120);
}

#[test]
fn sources_and_interval() {
    let toml = r#"
        interval_s = 60
        [github]
        repos = ["firma/app", "firma/web.v2"]
        [jira]
        site = "https://firma.atlassian.net"
        email = "ich@firma.example"
        projects = ["BAU", "WEB"]
        jql = "  "
    "#;
    let c = Config::parse(Some(toml), no_env).unwrap();
    assert_eq!(c.interval_s, 60);
    assert_eq!(c.github.unwrap().repos, ["firma/app", "firma/web.v2"]);
    let j = c.jira.unwrap();
    assert_eq!(j.projects, ["BAU", "WEB"]);
    assert_eq!(j.jql, None, "leere Abfrage zählt nicht");
    for broken in [
        "interval_s = 5",
        "[github]\nrepos = [\"nur-name\"]",
        "[github]\nrepos = [\"a/b/c\"]",
        "[github]\nrepos = [\"a b/c\"]",
        "[jira]\nsite = \"https://x\"\nemail = \"a@b\"\nprojects = [\"A) OR (B\"]",
    ] {
        assert!(
            matches!(
                Config::parse(Some(broken), no_env),
                Err(ConfigError::Wert(_))
            ),
            "{broken}"
        );
    }
}

#[test]
fn env_overwrites_file() {
    let env = |k: &str| match k {
        "COCKPIT_PORT" => Some("9100".to_string()),
        "COCKPIT_DB" => Some("/tmp/x.db".to_string()),
        _ => None,
    };
    let c = Config::parse(Some("port = 9000"), env).unwrap();
    assert_eq!(c.port, 9100);
    assert_eq!(c.db, PathBuf::from("/tmp/x.db"));
}

#[test]
fn errors_are_understandable() {
    let env_broken = |k: &str| (k == "COCKPIT_PORT").then(|| "abc".to_string());
    assert!(matches!(
        Config::parse(None, env_broken),
        Err(ConfigError::Wert(_))
    ));
    assert!(matches!(
        Config::parse(Some("port = "), no_env),
        Err(ConfigError::Toml(_))
    ));
    // Tippfehler in der Datei fallen auf, statt still ignoriert zu werden.
    assert!(matches!(
        Config::parse(Some("prot = 1"), no_env),
        Err(ConfigError::Toml(_))
    ));
    assert!(matches!(
        Config::parse(Some("port = 0"), no_env),
        Err(ConfigError::Wert(_))
    ));
    // Jira braucht Adresse und E-Mail.
    assert!(Config::parse(Some("[jira]\nsite = \"https://x\""), no_env).is_err());
}

#[test]
fn oauth_sections() {
    let toml = r#"
        [github_oauth]
        client_id = " Iv23abc "
        [atlassian]
        client_id = "atl123"
        site = "firma"
    "#;
    let c = Config::parse(Some(toml), no_env).unwrap();
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
        Config::parse(Some(toml), no_env)
            .unwrap()
            .jira
            .unwrap()
            .projects,
        ["BAU"]
    );
    for broken in [
        "[github_oauth]\nclient_id = \"\"",
        "[atlassian]\nclient_id = \"a b\"",
    ] {
        assert!(matches!(
            Config::parse(Some(broken), no_env),
            Err(ConfigError::Wert(_))
        ));
    }
    // Tippfehler im Abschnitt fallen auf (Secrets gehören nicht in die Datei).
    assert!(matches!(
        Config::parse(
            Some("[atlassian]\nclient_id = \"a\"\nclient_secret = \"x\""),
            no_env
        ),
        Err(ConfigError::Toml(_))
    ));
}

#[test]
fn seed_is_by_default_empty() {
    assert_eq!(Config::parse(None, no_env).unwrap().seed, Seed::Empty);
    assert_eq!(
        Config::parse(Some("seed = \"demo\""), no_env).unwrap().seed,
        Seed::Demo
    );
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| "leer".to_string());
    assert_eq!(
        Config::parse(Some("seed = \"demo\""), env).unwrap().seed,
        Seed::Empty,
        "die Umgebung geht der Datei vor"
    );
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| " Demo ".to_string());
    assert_eq!(Config::parse(None, env).unwrap().seed, Seed::Demo);
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| "viel".to_string());
    assert!(matches!(
        Config::parse(None, env),
        Err(ConfigError::Wert(_))
    ));
    assert!(Config::parse(Some("seed = \"viel\""), no_env).is_err());
}

#[test]
fn repo_and_dir_the_agents() {
    let toml = "[agent]\nrepo = \" firma/app \"\ndir = \"/opt/cockpit/agent\"";
    let c = Config::parse(Some(toml), no_env).unwrap();
    assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
    assert_eq!(c.agent_dir, Some(PathBuf::from("/opt/cockpit/agent")));
    let env = |k: &str| match k {
        "COCKPIT_AGENT_DIR" => Some("/andere/agent".to_string()),
        "COCKPIT_GIT_BASIS" => Some("file:///tmp/git/".to_string()),
        _ => None,
    };
    let c = Config::parse(Some(toml), env).unwrap();
    assert_eq!(c.agent_dir, Some(PathBuf::from("/andere/agent")));
    assert_eq!(c.git_base.as_deref(), Some("file:///tmp/git"));
    for broken in ["repo = \"firma\"", "repo = \"../x\"", "repo = \"a/b/c\""] {
        assert!(
            matches!(
                Config::parse(Some(&format!("[agent]\n{broken}")), no_env),
                Err(ConfigError::Wert(_))
            ),
            "{broken}"
        );
    }
    // Ohne Angabe nichts.
    let c = Config::parse(None, no_env).unwrap();
    assert_eq!((c.agent_repo, c.agent_dir, c.git_base), (None, None, None));
}

#[test]
fn endpoint_and_container() {
    let toml = r#"
        [agent]
        container = "Podman"
        provider = "Claude-Agent-SDK"
        [agent.endpoint]
        url = " http://localhost:4000/ "
        model = "claude-sonnet-5-5"
    "#;
    let c = Config::parse(Some(toml), no_env).unwrap();
    assert_eq!(c.provider, Provider::ClaudeAgentSdk);
    let e = c.endpoint.unwrap();
    assert_eq!(e.url.as_deref(), Some("http://localhost:4000"));
    assert_eq!(e.model.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(c.container.as_deref(), Some("podman"));
    assert_eq!(c.container_env, None);

    let env = |k: &str| (k == "COCKPIT_CONTAINER").then(|| "docker".to_string());
    assert_eq!(
        Config::parse(None, env).unwrap().container_env.as_deref(),
        Some("docker")
    );
    for broken in [
        "[agent.endpoint]\nurl = \"localhost:4000\"",
        "[agent.endpoint]\nurl = \"http://\"",
        "[agent.endpoint]\nurl = \"http://a b\"",
        "[agent]\ncontainer = \"nerdctl\"",
        "[agent]\nprovider = \"gpt\"",
        "[litellm]\nurl = \"localhost:4000\"",
    ] {
        assert!(
            matches!(
                Config::parse(Some(broken), no_env),
                Err(ConfigError::Wert(_))
            ),
            "{broken}"
        );
    }
    // Der Key gehört nicht in die Datei.
    for with_key in [
        "[agent.endpoint]\nurl = \"http://x\"\nkey = \"abc\"",
        "[litellm]\nurl = \"http://x\"\nkey = \"abc\"",
    ] {
        assert!(matches!(
            Config::parse(Some(with_key), no_env),
            Err(ConfigError::Toml(_))
        ));
    }
}

#[test]
fn direct_at_provider_has_no_address() {
    let c = Config::parse(
        Some("[agent.endpoint]\nmodel = \"claude-opus-5-5\""),
        no_env,
    )
    .unwrap();
    let e = c.endpoint.unwrap();
    assert_eq!((e.url, e.model.as_deref()), (None, Some("claude-opus-5-5")));
    // Leere Adresse = direkt; ganz leerer Abschnitt = nicht eingerichtet.
    let c = Config::parse(Some("[agent.endpoint]\nurl = \"\"\nmodel = \"m\""), no_env).unwrap();
    assert_eq!(c.endpoint.unwrap().url, None);
    assert_eq!(
        Config::parse(Some("[agent.endpoint]"), no_env)
            .unwrap()
            .endpoint,
        None
    );
    assert_eq!(
        Config::parse(None, no_env).unwrap().provider,
        Provider::ClaudeAgentSdk
    );
}

#[test]
fn known_tools_let_itself_add() {
    for (text, should) in [
        ("claude-code", Provider::ClaudeCode),
        ("codex", Provider::Codex),
    ] {
        let c = Config::parse(Some(&format!("[agent]\nprovider = \"{text}\"")), no_env).unwrap();
        assert_eq!(c.provider, should);
    }
}

#[test]
fn old_litellm_file_is_further_read_and_the_new_goes_before() {
    let alt = "[litellm]\nurl = \"http://localhost:4000/\"\nmodell = \"altes-modell\"\n[agent]\ncontainer = \"docker\"";
    let c = Config::parse(Some(alt), no_env).unwrap();
    let e = c.endpoint.unwrap();
    assert_eq!(e.url.as_deref(), Some("http://localhost:4000"));
    assert_eq!(e.model.as_deref(), Some("altes-modell"));
    assert_eq!(c.provider, Provider::ClaudeAgentSdk);
    assert_eq!(c.container.as_deref(), Some("docker"));

    let updated = "[agent.endpoint]\nurl = \"https://litellm.example\"\nmodel = \"neues-modell\"";
    let c = Config::parse(Some(updated), no_env).unwrap();
    assert_eq!(
        c.endpoint,
        Config::parse(Some(&format!("{alt}\n{updated}")), no_env)
            .unwrap()
            .endpoint
    );
    let e = c.endpoint.unwrap();
    assert_eq!(e.url.as_deref(), Some("https://litellm.example"));
    assert_eq!(e.model.as_deref(), Some("neues-modell"));
}

#[test]
fn dev_port_only_out_the_env() {
    assert_eq!(Config::parse(None, no_env).unwrap().dev_port, None);
    let env = |k: &str| (k == "COCKPIT_DEV_PORT").then(|| "8081".to_string());
    assert_eq!(Config::parse(None, env).unwrap().dev_port, Some(8081));
    let env = |k: &str| (k == "COCKPIT_DEV_PORT").then(|| "x".to_string());
    assert!(Config::parse(None, env).is_err());
    assert!(Config::parse(Some("dev_port = 8081"), no_env).is_err());
}

#[test]
fn older_german_spellings_are_still_read() {
    let old = r#"
        intervall_s = 90
        [agent]
        anbieter = "codex"
        [agent.endpunkt]
        url = "http://localhost:4000/"
        modell = "altes-modell"
    "#;
    let c = Config::parse(Some(old), no_env).unwrap();
    assert_eq!(c.interval_s, 90);
    assert_eq!(c.provider, Provider::Codex);
    let e = c.endpoint.unwrap();
    assert_eq!(e.url.as_deref(), Some("http://localhost:4000"));
    assert_eq!(e.model.as_deref(), Some("altes-modell"));
    // The old and the new key of the same value together are a duplicate, not a silent choice.
    assert!(Config::parse(Some("interval_s = 60\nintervall_s = 90"), no_env).is_err());
    // Older environment spellings: `COCKPIT_GIT_BASIS` and the seed `leer`.
    let env = |k: &str| match k {
        "COCKPIT_GIT_BASIS" => Some("http://git.example/".to_string()),
        "COCKPIT_SEED" => Some("leer".to_string()),
        _ => None,
    };
    let c = Config::parse(None, env).unwrap();
    assert_eq!(c.git_base.as_deref(), Some("http://git.example"));
    assert_eq!(c.seed, Seed::Empty);
    // The new names win over the old ones.
    let env = |k: &str| match k {
        "COCKPIT_GIT_BASE" => Some("http://new.example".to_string()),
        "COCKPIT_GIT_BASIS" => Some("http://old.example".to_string()),
        "COCKPIT_SEED" => Some("empty".to_string()),
        _ => None,
    };
    let c = Config::parse(None, env).unwrap();
    assert_eq!(c.git_base.as_deref(), Some("http://new.example"));
    assert_eq!(c.seed, Seed::Empty);
}
