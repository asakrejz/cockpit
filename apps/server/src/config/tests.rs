use super::*;

fn no_env(_: &str) -> Option<String> {
    None
}

#[test]
fn defaults() {
    let c = Config::parse(None, no_env).unwrap();
    assert_eq!(c.port, 8787);
    assert_eq!(c.dist, PathBuf::from("apps/cockpit/dist"));
    assert!(c.db.ends_with("config/cockpit.db"));
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
    assert_eq!(j.jql, None, "an empty query does not count");
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
                Err(ConfigError::InvalidValue(_))
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
        Err(ConfigError::InvalidValue(_))
    ));
    assert!(matches!(
        Config::parse(Some("port = "), no_env),
        Err(ConfigError::Toml(_))
    ));
    // Typos in the file stand out instead of being silently ignored.
    assert!(matches!(
        Config::parse(Some("prot = 1"), no_env),
        Err(ConfigError::UnknownKey { .. })
    ));
    assert!(matches!(
        Config::parse(Some("port = 0"), no_env),
        Err(ConfigError::InvalidValue(_))
    ));
    // Jira needs address and email.
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
    // [atlassian] alone sets up Jira, without site and email.
    let j = c.jira.unwrap();
    assert!(j.site.is_empty() && j.email.is_empty() && j.projects.is_empty());

    // Projects stay configurable.
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
            Err(ConfigError::InvalidValue(_))
        ));
    }
    // Typos in the section stand out (secrets do not belong in the file).
    assert!(matches!(
        Config::parse(
            Some("[atlassian]\nclient_id = \"a\"\nclient_secret = \"x\""),
            no_env
        ),
        Err(ConfigError::UnknownKey { .. })
    ));
}

#[test]
fn seed_is_by_default_empty() {
    assert_eq!(Config::parse(None, no_env).unwrap().seed, Seed::Empty);
    assert_eq!(
        Config::parse(Some("seed = \"demo\""), no_env).unwrap().seed,
        Seed::Demo
    );
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| "empty".to_string());
    assert_eq!(
        Config::parse(Some("seed = \"demo\""), env).unwrap().seed,
        Seed::Empty,
        "the environment goes before the file"
    );
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| " Demo ".to_string());
    assert_eq!(Config::parse(None, env).unwrap().seed, Seed::Demo);
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| "viel".to_string());
    assert!(matches!(
        Config::parse(None, env),
        Err(ConfigError::InvalidValue(_))
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
        "COCKPIT_GIT_BASE" => Some("file:///tmp/git/".to_string()),
        _ => None,
    };
    let c = Config::parse(Some(toml), env).unwrap();
    assert_eq!(c.agent_dir, Some(PathBuf::from("/andere/agent")));
    assert_eq!(c.git_base.as_deref(), Some("file:///tmp/git"));
    for broken in ["repo = \"firma\"", "repo = \"../x\"", "repo = \"a/b/c\""] {
        assert!(
            matches!(
                Config::parse(Some(&format!("[agent]\n{broken}")), no_env),
                Err(ConfigError::InvalidValue(_))
            ),
            "{broken}"
        );
    }
    // Nothing without a value.
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
    let io = |v: &'static str| move |k: &str| (k == "COCKPIT_AGENT_IO").then(|| v.to_string());
    assert_eq!(Config::parse(None, no_env).unwrap().agent_io, None);
    assert_eq!(
        Config::parse(None, io(" Copy "))
            .unwrap()
            .agent_io
            .as_deref(),
        Some("copy")
    );
    assert!(matches!(
        Config::parse(None, io("mount")),
        Err(ConfigError::InvalidValue(_))
    ));
    for broken in [
        "[agent.endpoint]\nurl = \"localhost:4000\"",
        "[agent.endpoint]\nurl = \"http://\"",
        "[agent.endpoint]\nurl = \"http://a b\"",
        "[agent]\ncontainer = \"nerdctl\"",
        "[agent]\nprovider = \"gpt\"",
    ] {
        assert!(
            matches!(
                Config::parse(Some(broken), no_env),
                Err(ConfigError::InvalidValue(_))
            ),
            "{broken}"
        );
    }
    // The key does not belong in the file.
    assert!(matches!(
        Config::parse(
            Some("[agent.endpoint]\nurl = \"http://x\"\nkey = \"abc\""),
            no_env
        ),
        Err(ConfigError::UnknownKey { .. })
    ));
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
    // Empty address = direct; a completely empty section = not set up.
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
fn dev_port_only_out_the_env() {
    assert_eq!(Config::parse(None, no_env).unwrap().dev_port, None);
    let env = |k: &str| (k == "COCKPIT_DEV_PORT").then(|| "8081".to_string());
    assert_eq!(Config::parse(None, env).unwrap().dev_port, Some(8081));
    let env = |k: &str| (k == "COCKPIT_DEV_PORT").then(|| "x".to_string());
    assert!(Config::parse(None, env).is_err());
    assert!(Config::parse(Some("dev_port = 8081"), no_env).is_err());
}

/// The place of an unknown key: `(section, key)`.
fn unknown_key(text: &str) -> (String, String) {
    match Config::parse(Some(text), no_env) {
        Err(ConfigError::UnknownKey { section, key, .. }) => (section, key),
        other => panic!("{text}: {other:?}"),
    }
}

#[test]
fn names_that_do_not_exist_any_more_are_refused_with_key_and_section() {
    for (text, section, key) in [
        (
            "[litellm]\nurl = \"http://localhost:4000\"\nmodell = \"m\"",
            "",
            "litellm",
        ),
        ("[agent]\nanbieter = \"codex\"", "agent", "anbieter"),
        (
            "[agent.endpunkt]\nurl = \"http://a:1\"",
            "agent",
            "endpunkt",
        ),
        (
            "[agent.endpoint]\nurl = \"http://a:1\"\nmodell = \"m\"",
            "agent.endpoint",
            "modell",
        ),
        ("intervall_s = 90", "", "intervall_s"),
        (
            "[agent]\nendpoint = { modell = \"m\" }",
            "agent.endpoint",
            "modell",
        ),
        (
            "[[agent.repos]]\nrepo = \"acme/app\"\nzweig = \"x\"",
            "agent.repos",
            "zweig",
        ),
        (
            "[jira]\nsite = \"https://x\"\nemail = \"a@b\"\nnope = 1",
            "jira",
            "nope",
        ),
    ] {
        assert_eq!(unknown_key(text), (section.into(), key.into()), "{text}");
    }
}

#[test]
fn an_unknown_key_message_names_key_section_and_file_in_both_languages() {
    use cockpit_i18n::Lang;
    let file = Path::new("/srv/cockpit/config.toml");
    let error = |text: &str| Config::parse(Some(text), no_env).unwrap_err().in_file(file);

    let e = error("[agent]\nanbieter = \"codex\"");
    for lang in [Lang::De, Lang::En] {
        let text = e.msg().render(lang);
        for part in [
            "anbieter",
            "[agent]",
            "/srv/cockpit/config.toml",
            "docs/configuration.md",
        ] {
            assert!(text.contains(part), "{lang:?}: {part} missing in {text}");
        }
    }
    assert!(e.msg().render(Lang::De).contains("Benenne"));
    assert!(e.msg().render(Lang::En).contains("Rename"));
    // The log line at start is the English text.
    assert_eq!(e.to_string(), e.msg().render(Lang::En));

    // A whole section at the top (`[litellm]`) has no section to name.
    let e = error("[litellm]\nurl = \"http://localhost:4000\"");
    for lang in [Lang::De, Lang::En] {
        let text = e.msg().render(lang);
        assert!(
            text.contains("litellm") && text.contains("/srv/cockpit/config.toml"),
            "{text}"
        );
        assert!(!text.contains("[]"), "{text}");
    }

    // Without the path of the file the message names the default file.
    let e = Config::parse(Some("prot = 1"), no_env).unwrap_err();
    assert!(e.to_string().contains("config.toml"), "{e}");
    // Only an unknown key becomes this error; a broken line stays a parse error.
    assert!(matches!(
        Config::parse(Some("port = "), no_env),
        Err(ConfigError::Toml(_))
    ));
}

#[test]
fn removed_environment_spellings_are_not_read() {
    let env = |k: &str| (k == "COCKPIT_GIT_BASIS").then(|| "http://git.example/".to_string());
    assert_eq!(Config::parse(None, env).unwrap().git_base, None);
    let env = |k: &str| (k == "COCKPIT_SEED").then(|| "leer".to_string());
    assert!(matches!(
        Config::parse(None, env),
        Err(ConfigError::InvalidValue(_))
    ));
    let env = |k: &str| (k == "COCKPIT_GIT_BASE").then(|| "http://git.example/".to_string());
    assert_eq!(
        Config::parse(None, env).unwrap().git_base.as_deref(),
        Some("http://git.example")
    );
}

#[test]
fn a_file_with_the_current_names_loads() {
    let current = r#"
        port = 9000
        interval_s = 90
        seed = "empty"
        [github]
        repos = ["acme/app"]
        [jira]
        site = "https://acme.atlassian.net"
        email = "me@acme.example"
        projects = ["BAU"]
        [agent]
        provider = "codex"
        container = "docker"
        repo = "acme/app"
        [agent.endpoint]
        url = "http://localhost:4000/"
        model = "some-model"
        [[agent.repos]]
        repo = "acme/app"
        base_branch = "develop"
        [server]
        webhooks = true
        [sync]
        max_prs_total = 100
    "#;
    let c = Config::parse(Some(current), no_env).unwrap();
    assert_eq!(
        (c.port, c.interval_s, c.provider),
        (9000, 90, Provider::Codex)
    );
    assert_eq!(c.container.as_deref(), Some("docker"));
    let e = c.endpoint.unwrap();
    assert_eq!(e.url.as_deref(), Some("http://localhost:4000"));
    assert_eq!(e.model.as_deref(), Some("some-model"));
    assert_eq!(c.agent_repos[0].base_branch, "develop");
    assert!(c.webhooks);
}

#[test]
fn environment_addresses_are_checked_with_catalog_messages() {
    use cockpit_i18n::Lang;
    for (name, value, part) in [
        ("COCKPIT_BIND", "not-an-ip", "COCKPIT_BIND=not-an-ip"),
        ("COCKPIT_PUBLIC_PORT", "x", "COCKPIT_PUBLIC_PORT=x"),
        ("COCKPIT_PUBLIC_PORT", "0", "COCKPIT_PUBLIC_PORT=0"),
    ] {
        let env = |k: &str| (k == name).then(|| value.to_string());
        let Err(ConfigError::InvalidValue(m)) = Config::parse(None, env) else {
            panic!("{name}={value} must be refused");
        };
        assert_ne!(m.code, "common.raw", "{name} has a catalog key");
        for lang in [Lang::De, Lang::En] {
            assert!(
                m.render(lang).contains(part),
                "{lang:?}: {}",
                m.render(lang)
            );
        }
    }
    let env = |k: &str| match k {
        "COCKPIT_BIND" => Some(" 0.0.0.0 ".to_string()),
        "COCKPIT_PUBLIC_PORT" => Some("18787".to_string()),
        _ => None,
    };
    let c = Config::parse(None, env).unwrap();
    assert_eq!(
        (c.bind.to_string(), c.public_port),
        ("0.0.0.0".into(), Some(18787))
    );
}

#[test]
fn agent_repos_with_base_branch_and_pattern() {
    let toml = r#"
        [agent]
        repo = "firma/app"

        [[agent.repos]]
        repo = "firma/web"
        base_branch = "develop"

        [[agent.repos]]
        repo = "firma/app"
    "#;
    let c = Config::parse(Some(toml), no_env).unwrap();
    assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
    assert_eq!(c.agent_repos.len(), 2);
    let web = &c.agent_repos[0];
    assert_eq!(web.repo, "firma/web");
    assert_eq!(web.base_branch, "develop");
    assert_eq!(c.agent_repos[1].base_branch, "main");
}

#[test]
fn a_projects_key_of_an_agent_repo_is_accepted_and_has_no_effect() {
    // Whatever the value is (a list, a bad key, a text): the file still starts and reads
    // exactly like the file without the key.
    let plain = "[[agent.repos]]\nrepo = \"a/b\"\nbase_branch = \"dev\"\n\
                 [[agent.repos]]\nrepo = \"a/c\"";
    let expected = Config::parse(Some(plain), no_env).unwrap().agent_repos;
    for value in ["[\"1X\", \"web\", \"WEB\"]", "[]", "\"WEB\"", "3"] {
        let toml = format!(
            "[[agent.repos]]\nrepo = \"a/b\"\nbase_branch = \"dev\"\nprojects = {value}\n\
             [[agent.repos]]\nrepo = \"a/c\"\nprojects = [\"WEB\"]"
        );
        let c = Config::parse(Some(&toml), no_env).unwrap();
        assert_eq!(c.agent_repos, expected, "{value}");
    }
}

#[test]
fn the_branch_pattern_defaults_and_can_be_set_per_repo() {
    let toml = r#"
        [[agent.repos]]
        repo = "firma/web"
        branch_pattern = "feature/{ticket}-{slug}"

        [[agent.repos]]
        repo = "firma/app"
    "#;
    let c = Config::parse(Some(toml), no_env).unwrap();
    assert_eq!(c.agent_repos[0].branch_pattern, "feature/{ticket}-{slug}");
    assert_eq!(c.agent_repos[1].branch_pattern, "claude/{ticket}-{slug}");
}

#[test]
fn an_invalid_branch_pattern_is_rejected_when_the_config_is_read() {
    for bad in [
        "main",
        "",
        "   ",
        "feature/{slug}",
        "{ticket}{slug}",
        "feature/{nope}-{ticket}",
        "feature/{ticket}/",
        "a b/{ticket}",
    ] {
        let toml = format!("[[agent.repos]]\nrepo = \"a/b\"\nbranch_pattern = \"{bad}\"");
        let result = Config::parse(Some(&toml), no_env);
        // An empty value is the same as none: it falls back to the default.
        if bad.trim().is_empty() {
            assert_eq!(
                result.unwrap().agent_repos[0].branch_pattern,
                "claude/{ticket}-{slug}"
            );
            continue;
        }
        assert!(
            matches!(&result, Err(ConfigError::InvalidValue(m)) if m.render(cockpit_i18n::Lang::De).contains("branch_pattern")),
            "{bad}: {result:?}"
        );
    }
}

#[test]
fn the_old_single_repo_config_still_reads_and_has_no_list() {
    let c = Config::parse(
        Some("[agent]\nrepo = \"firma/app\"\n[github]\nrepos = [\"firma/app\"]"),
        no_env,
    )
    .unwrap();
    assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
    assert!(c.agent_repos.is_empty());
    assert!(Config::parse(None, no_env).unwrap().agent_repos.is_empty());
}

#[test]
fn broken_agent_repos_are_rejected_with_a_reason() {
    for broken in [
        "[[agent.repos]]\nrepo = \"firma\"",
        "[[agent.repos]]\nrepo = \"a/b\"\n[[agent.repos]]\nrepo = \"a/b\"",
        "[[agent.repos]]\nrepo = \"a/b\"\nbase_branch = \"-x\"",
        "[[agent.repos]]\nrepo = \"a/b\"\nunknown = 1",
    ] {
        assert!(
            matches!(
                Config::parse(Some(broken), no_env),
                Err(ConfigError::InvalidValue(_) | ConfigError::UnknownKey { .. })
            ),
            "{broken}"
        );
    }
}

#[test]
fn webhooks_are_off_by_default() {
    let c = Config::parse(None, no_env).unwrap();
    assert!(!c.webhooks);
    assert_eq!(c.public_url, None);
    assert_eq!(c.webhook_fallback_interval_s, None);
    // 5x the interval, at most 10 minutes.
    assert_eq!(c.webhook_fallback_s(), 600);
    // A file without the webhook settings reads unchanged.
    let old = "interval_s = 60\n[github]\nrepos = [\"a/b\"]\n";
    let c = Config::parse(Some(old), no_env).unwrap();
    assert!(!c.webhooks);
    assert_eq!(c.interval_s, 60);
    assert_eq!(c.webhook_fallback_s(), 300);
}

#[test]
fn webhook_settings_from_the_file() {
    let toml = r#"
        interval_s = 120
        [server]
        webhooks = true
        public_url = "https://cockpit.example/"
        [sync]
        webhook_fallback_interval_s = 900
    "#;
    let c = Config::parse(Some(toml), no_env).unwrap();
    assert!(c.webhooks);
    assert_eq!(c.public_url.as_deref(), Some("https://cockpit.example"));
    assert_eq!(c.webhook_fallback_s(), 900);
    // `[server]` alone, without an address.
    let c = Config::parse(Some("[server]\nwebhooks = true\n"), no_env).unwrap();
    assert!(c.webhooks && c.public_url.is_none());
}

#[test]
fn webhook_settings_are_validated() {
    for toml in [
        "[server]\npublic_url = \"http://cockpit.example\"\n",
        "[server]\npublic_url = \"https://cockpit.example/x?a=b\"\n",
        "[server]\npublic_url = \"https://user@cockpit.example\"\n",
        "[server]\nunknown = 1\n",
        "interval_s = 120\n[sync]\nwebhook_fallback_interval_s = 60\n",
        "[sync]\nwebhook_fallback_interval_s = 99999\n",
    ] {
        assert!(Config::parse(Some(toml), no_env).is_err(), "{toml}");
    }
    assert_eq!(fallback_s(30, None), 150);
    assert_eq!(fallback_s(300, None), 600);
    assert_eq!(
        fallback_s(900, None),
        900,
        "never shorter than the interval"
    );
}

#[test]
fn pull_request_limits_default_and_come_from_the_sync_section() {
    let c = Config::parse(None, no_env).unwrap();
    assert_eq!(
        (c.github_pr_limits.per_repo, c.github_pr_limits.total),
        (200, 500)
    );
    let toml = "[sync]\nmax_prs_per_repo = 50\nmax_prs_total = 120\n";
    let c = Config::parse(Some(toml), no_env).unwrap();
    assert_eq!(
        (c.github_pr_limits.per_repo, c.github_pr_limits.total),
        (50, 120)
    );
    // One value alone keeps the other at its default.
    let c = Config::parse(Some("[sync]\nmax_prs_total = 90\n"), no_env).unwrap();
    assert_eq!(
        (c.github_pr_limits.per_repo, c.github_pr_limits.total),
        (200, 90)
    );
    for bad in [
        "[sync]\nmax_prs_per_repo = 0\n",
        "[sync]\nmax_prs_total = 100000\n",
        "[sync]\nmax_prs_per_repo = -1\n",
    ] {
        assert!(Config::parse(Some(bad), no_env).is_err(), "{bad}");
    }
}

#[test]
fn the_pull_request_language_is_english_unless_a_repo_says_german() {
    let toml = r#"
        [[agent.repos]]
        repo = "firma/web"
        pr_language = "de"

        [[agent.repos]]
        repo = "firma/app"
    "#;
    let c = Config::parse(Some(toml), no_env).unwrap();
    assert_eq!(c.agent_repos[0].pr_language, cockpit_i18n::Lang::De);
    assert_eq!(c.agent_repos[1].pr_language, cockpit_i18n::Lang::En);
    let bad = "[[agent.repos]]\nrepo = \"a/b\"\npr_language = \"fr\"";
    assert!(matches!(
        Config::parse(Some(bad), no_env),
        Err(ConfigError::InvalidValue(_))
    ));
}

#[test]
fn bind_and_public_port_only_out_the_env() {
    let c = Config::parse(None, no_env).unwrap();
    assert!(c.bind.is_loopback());
    assert_eq!(c.public_port, None);
    let env = |k: &str| match k {
        "COCKPIT_BIND" => Some("0.0.0.0".into()),
        "COCKPIT_PUBLIC_PORT" => Some("18787".into()),
        _ => None,
    };
    let c = Config::parse(None, env).unwrap();
    assert!(!c.bind.is_loopback());
    assert_eq!(c.public_port, Some(18787));
    let bad = |k: &str| (k == "COCKPIT_BIND").then(|| "everywhere".to_string());
    assert!(Config::parse(None, bad).is_err());
    assert!(Config::parse(Some("bind = \"0.0.0.0\""), no_env).is_err());
}

#[test]
fn github_projects_are_read_with_defaults_and_validated() {
    use cockpit_core::{ProjectRef, TicketsFrom};
    // No section: Jira only.
    assert!(
        Config::parse(None, no_env)
            .unwrap()
            .github_projects
            .is_none()
    );
    // A section without `tickets_from` means both.
    let c = Config::parse(
        Some("[github_projects]\nprojects = [\"acme/3\", \"acme/3\", \"mkeller/1\"]\nstatus_field = \" Stage \"\n"),
        no_env,
    )
    .unwrap();
    let p = c.github_projects.unwrap();
    assert_eq!(p.tickets_from, TicketsFrom::Both);
    assert_eq!(
        p.projects,
        [ProjectRef::new("acme", 3), ProjectRef::new("mkeller", 1)]
    );
    assert_eq!(p.status_field.as_deref(), Some("Stage"));
    assert_eq!(p.team_field, None);
    let c = Config::parse(
        Some("[github_projects]\ntickets_from = \"github_projects\"\n"),
        no_env,
    )
    .unwrap();
    assert_eq!(
        c.github_projects.unwrap().tickets_from,
        TicketsFrom::GitHubProjects
    );
    // Mistakes are named.
    assert!(Config::parse(Some("[github_projects]\ntickets_from = \"jra\"\n"), no_env).is_err());
    assert!(Config::parse(Some("[github_projects]\nprojects = [\"acme\"]\n"), no_env).is_err());
    assert!(Config::parse(Some("[github_projects]\nunknown_key = 1\n"), no_env).is_err());
}
