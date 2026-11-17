use super::*;

#[test]
fn change_keeps_comments_and_other_values() {
    let (updated, c) = change(WITH_COMMENTS, &change_github(&["firma/app", "firma/web"])).unwrap();
    assert!(updated.contains("# Meine Einstellungen"), "{updated}");
    assert!(updated.contains("port = 8787 # Standard"), "{updated}");
    assert!(
        updated.contains("# Diese Repos gleicht Cockpit ab"),
        "{updated}"
    );
    assert!(updated.contains("# alt"), "{updated}");
    assert_eq!(c.github.unwrap().repos, vec!["firma/app", "firma/web"]);
}

#[test]
fn change_puts_sections_in_a_empty_file_at() {
    let a = ConfigChange {
        atlassian: Some(ClientInput {
            client_id: " 7dNimh ".into(),
        }),
        github_oauth: Some(ClientInput {
            client_id: "Iv23li".into(),
        }),
        jira: Some(JiraInput {
            projects: Some(vec!["bau".into(), "web".into()]),
            ..Default::default()
        }),
        endpoint: Some(EndpointInput {
            provider: Some(Provider::ClaudeAgentSdk),
            url: Some("localhost:4000/".into()),
            model: Some("claude-sonnet-5-5".into()),
        }),
        ..Default::default()
    };
    let (updated, c) = change("", &a).unwrap();
    assert_eq!(c.atlassian.unwrap().client_id, "7dNimh");
    assert_eq!(c.github_oauth.unwrap().client_id, "Iv23li");
    assert_eq!(c.jira.unwrap().projects, vec!["BAU", "WEB"]);
    let l = c.endpoint.unwrap();
    assert_eq!(l.url.as_deref(), Some("http://localhost:4000"));
    assert_eq!(l.model.as_deref(), Some("claude-sonnet-5-5"));
    // The new format, no `[litellm]`.
    assert!(
        updated.contains("[agent.endpoint]") && !updated.contains("litellm"),
        "{updated}"
    );
    assert!(
        updated.contains("provider = \"claude-agent-sdk\""),
        "{updated}"
    );
    // The result is readable again as at start.
    assert!(Config::parse(Some(&updated), |_| None).is_ok());
}

#[test]
fn change_adopts_a_older_litellm_and_keeps_agent_values() {
    let alt = "[litellm]\nurl = \"http://a:1\"\nmodell = \"altes-modell\"\n[agent]\ncontainer = \"podman\" # meins\nrepo = \"firma/app\"\n";
    // Only change the model: the old address stays, `[litellm]` moves.
    let a = ConfigChange {
        endpoint: Some(EndpointInput {
            model: Some("neues-modell".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (updated, c) = change(alt, &a).unwrap();
    assert!(!updated.contains("[litellm]"), "{updated}");
    assert!(
        updated.contains("container = \"podman\" # meins"),
        "{updated}"
    );
    let e = c.endpoint.unwrap();
    assert_eq!(e.url.as_deref(), Some("http://a:1"));
    assert_eq!(e.model.as_deref(), Some("neues-modell"));
    assert_eq!(c.container.as_deref(), Some("podman"));
    assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
}

#[test]
fn change_migrates_all_older_spellings_to_the_current_ones() {
    // `[agent] anbieter`, `[agent.endpunkt]` with `modell`, plus a `[litellm]` that loses.
    let old = "[litellm]\nurl = \"http://x:9\"\nmodell = \"litellm-model\"\n[agent]\nanbieter = \"codex\"\n[agent.endpunkt]\nurl = \"http://a:1\"\nmodell = \"m\"\n";
    let a = ConfigChange {
        endpoint: Some(EndpointInput::default()),
        ..Default::default()
    };
    let (updated, c) = change(old, &a).unwrap();
    for gone in ["litellm", "anbieter", "endpunkt", "modell"] {
        assert!(!updated.contains(gone), "{gone} in {updated}");
    }
    assert!(updated.contains("provider = \"codex\""), "{updated}");
    assert!(updated.contains("[agent.endpoint]"), "{updated}");
    let e = c.endpoint.unwrap();
    assert_eq!(c.provider, Provider::Codex);
    assert_eq!(
        (e.url.as_deref(), e.model.as_deref()),
        (Some("http://a:1"), Some("m"))
    );
    // The result is stable: a second change does not move anything.
    assert_eq!(change(&updated, &a).unwrap().0, updated);
}

#[test]
fn change_switches_on_direct_without_address() {
    let start = "[agent.endpunkt]\nurl = \"http://a:1\"\nmodell = \"m\"\n";
    let a = ConfigChange {
        endpoint: Some(EndpointInput {
            provider: Some(Provider::ClaudeAgentSdk),
            url: Some(String::new()),
            model: Some("claude-opus-5-5".into()),
        }),
        ..Default::default()
    };
    let (updated, c) = change(start, &a).unwrap();
    assert!(!updated.contains("url"), "{updated}");
    let e = c.endpoint.unwrap();
    assert_eq!((e.url, e.model.as_deref()), (None, Some("claude-opus-5-5")));
    // Claude Code and Codex can be entered like the SDK.
    let a = ConfigChange {
        endpoint: Some(EndpointInput {
            provider: Some(Provider::Codex),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(change("", &a).unwrap().1.provider, Provider::Codex);
}

#[test]
fn change_removes_with_empty_evaluate() {
    let start = "[atlassian]\nclient_id = \"x\"\n[agent.endpunkt]\nurl = \"http://a:1\"\nmodell = \"m\"\n[github]\nrepos = [\"a/b\"]\n";
    let a = ConfigChange {
        atlassian: Some(ClientInput::default()),
        endpoint: Some(EndpointInput {
            url: Some(String::new()),
            model: Some(String::new()),
            ..Default::default()
        }),
        github: Some(GitHubInput::default()),
        ..Default::default()
    };
    let (updated, c) = change(start, &a).unwrap();
    assert!(c.atlassian.is_none() && c.endpoint.is_none());
    assert!(!updated.contains("agent"), "{updated}");
    assert!(c.github.is_none(), "{updated}");
}

#[test]
fn change_checks_like_the_start() {
    // Jira without OAuth needs site and email.
    let only_projects = ConfigChange {
        jira: Some(JiraInput {
            projects: Some(vec!["BAU".into()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(change("", &only_projects).is_err());
    let token = ConfigChange {
        jira: Some(JiraInput {
            site: Some("firma".into()),
            email: Some("ich@firma.example".into()),
            projects: None,
        }),
        ..Default::default()
    };
    let (_, c) = change("", &token).unwrap();
    let j = c.jira.unwrap();
    assert_eq!(j.site, "https://firma.atlassian.net");
    assert_eq!(j.email, "ich@firma.example");

    for bad in [
        change_github(&["kein-repo"]),
        ConfigChange {
            jira: Some(JiraInput {
                email: Some("keine-adresse".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ConfigChange {
            endpoint: Some(EndpointInput {
                url: Some("ftp://x".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ConfigChange {
            github_oauth: Some(ClientInput {
                client_id: "mit leerzeichen".into(),
            }),
            ..Default::default()
        },
    ] {
        assert!(change("", &bad).is_err(), "{bad:?}");
    }
    // A broken file is not overwritten.
    assert!(change("port = [", &change_github(&["a/b"])).is_err());
}

#[test]
fn write_is_atomic_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unter").join("config.toml");
    atomic_write(&path, "a = 1\n").unwrap();
    atomic_write(&path, "a = 2\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 2\n");
    let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn change_writes_agent_repos_and_keeps_the_rest_of_agent() {
    let old = "[agent]\ncontainer = \"podman\" # meins\nrepo = \"alt/repo\"\n";
    let (updated, c) = change(old, &agent_repos_change()).unwrap();
    assert!(
        updated.contains("container = \"podman\" # meins"),
        "{updated}"
    );
    assert!(updated.contains("[[agent.repos]]"), "{updated}");
    assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
    assert_eq!(c.agent_repos.len(), 2);
    assert_eq!(c.agent_repos[1].projects, ["WEB"]);
    assert_eq!(c.agent_repos[1].base_branch, "develop");
    // Writing again gives the same file, and an empty list removes everything.
    let (again, _) = change(&updated, &agent_repos_change()).unwrap();
    assert_eq!(again, updated);
    let none = ConfigChange {
        agent_repos: Some(Default::default()),
        ..Default::default()
    };
    let (cleared, c) = change(&updated, &none).unwrap();
    assert!(
        !cleared.contains("repos") && !cleared.contains("repo ="),
        "{cleared}"
    );
    assert!(c.agent_repos.is_empty() && c.agent_repo.is_none());
    assert!(cleared.contains("container"));
}

#[test]
fn change_rejects_a_default_outside_the_list_and_double_projects() {
    use cockpit_core::AgentRepo;
    use cockpit_core::api::AgentReposInput;
    let outside = ConfigChange {
        agent_repos: Some(AgentReposInput {
            repos: vec![AgentRepo::new("a/b")],
            default: Some("c/d".into()),
        }),
        ..Default::default()
    };
    assert!(change("", &outside).unwrap_err().contains("Standard-Repo"));
    let double = ConfigChange {
        agent_repos: Some(AgentReposInput {
            repos: ["a/b", "a/c"]
                .map(|r| AgentRepo {
                    projects: vec!["WEB".into()],
                    ..AgentRepo::new(r)
                })
                .to_vec(),
            default: None,
        }),
        ..Default::default()
    };
    assert!(change("", &double).unwrap_err().contains("WEB"));
}

#[test]
fn change_writes_the_server_section_and_keeps_the_rest() {
    use cockpit_core::api::WebhooksInput;
    let a = ConfigChange {
        webhooks: Some(WebhooksInput {
            enabled: Some(true),
            public_url: Some(" https://cockpit.example/ ".into()),
        }),
        ..Default::default()
    };
    let (updated, c) = change(WITH_COMMENTS, &a).unwrap();
    assert!(updated.contains("# Meine Einstellungen"), "{updated}");
    assert!(updated.contains("webhooks = true"), "{updated}");
    assert!(
        updated.contains("public_url = \"https://cockpit.example\""),
        "{updated}"
    );
    assert!(c.webhooks);
    assert_eq!(c.public_url.as_deref(), Some("https://cockpit.example"));
    // Switch off, remove the address: the section stays with `webhooks = false` only.
    let off = ConfigChange {
        webhooks: Some(WebhooksInput {
            enabled: Some(false),
            public_url: Some(String::new()),
        }),
        ..Default::default()
    };
    let (updated, c) = change(&updated, &off).unwrap();
    assert!(!c.webhooks && c.public_url.is_none(), "{updated}");
    assert!(!updated.contains("public_url"), "{updated}");
    // A bad address is refused and nothing is written.
    let bad = ConfigChange {
        webhooks: Some(WebhooksInput {
            enabled: None,
            public_url: Some("http://cockpit.example".into()),
        }),
        ..Default::default()
    };
    assert!(change(WITH_COMMENTS, &bad).is_err());
}
