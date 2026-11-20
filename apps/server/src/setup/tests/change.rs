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
    assert!(
        change("", &outside)
            .unwrap_err()
            .de()
            .contains("Standard-Repo")
    );
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
    assert!(change("", &double).unwrap_err().de().contains("WEB"));
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

const OLD_GITHUB: &str = "[github]\n# nur lesen\napi_url = \"https://ghe.example/api/v3\"\nrepos = [\"firma/app\", \"firma/alt\"]\n\n[agent]\nrepo = \"firma/app\"\n";

#[test]
fn an_old_config_with_github_repos_still_reads_and_syncs() {
    let c = Config::parse(Some(OLD_GITHUB), |_| None).unwrap();
    assert_eq!(c.github.unwrap().repos, ["firma/app", "firma/alt"]);
    assert!(c.agent_repos.is_empty());
    assert_eq!(c.agent_repo.as_deref(), Some("firma/app"));
}

#[test]
fn agent_repos_alone_are_also_the_repos_of_the_sync() {
    let text = "[[agent.repos]]\nrepo = \"firma/web\"\n\n[[agent.repos]]\nrepo = \"firma/app\"\nbase_branch = \"develop\"\n";
    let c = Config::parse(Some(text), |_| None).unwrap();
    let g = c.github.unwrap();
    assert_eq!(g.repos, ["firma/web", "firma/app"]);
    assert_eq!(g.api_url, crate::config::DEFAULT_GITHUB_API);
    // Both spellings together: no duplicates.
    let both = format!("[github]\nrepos = [\"firma/app\", \"firma/zusatz\"]\n\n{text}");
    let c = Config::parse(Some(&both), |_| None).unwrap();
    assert_eq!(
        c.github.unwrap().repos,
        ["firma/app", "firma/zusatz", "firma/web"]
    );
}

#[test]
fn saving_the_list_keeps_the_watched_repos_of_the_github_section() {
    use cockpit_core::AgentRepo;
    use cockpit_core::api::AgentReposInput;
    let a = ConfigChange {
        agent_repos: Some(AgentReposInput {
            repos: vec![
                AgentRepo::new("firma/app"),
                AgentRepo {
                    base_branch: "develop".into(),
                    projects: vec!["WEB".into()],
                    ..AgentRepo::new("firma/neu")
                },
            ],
            default: Some("firma/app".into()),
        }),
        ..Default::default()
    };
    let (text, c) = change(OLD_GITHUB, &a).unwrap();
    // The watched repos of `[github]` are a list of their own and stay, with the address.
    assert!(text.contains("repos = [\"firma/app\""), "{text}");
    assert!(text.contains("api_url = \"https://ghe.example/api/v3\""));
    assert!(text.contains("[[agent.repos]]") && text.contains("# nur lesen"));
    let g = c.github.unwrap();
    assert_eq!(g.api_url, "https://ghe.example/api/v3");
    assert!(g.repos.contains(&"firma/neu".to_string()), "{:?}", g.repos);
    // The written file reads back the same.
    let again = Config::parse(Some(&text), |_| None).unwrap();
    assert_eq!(again.agent_repos, c.agent_repos);
    assert_eq!(again.agent_repos[1].base_branch, "develop");
    assert_eq!(again.github.unwrap().repos, g.repos);
}

#[test]
fn a_repo_stored_on_first_use_is_not_watched_and_reads_back() {
    use cockpit_core::AgentRepo;
    use cockpit_core::api::AgentReposInput;
    let a = ConfigChange {
        agent_repos: Some(AgentReposInput {
            repos: vec![AgentRepo {
                base_branch: "develop".into(),
                watch: false,
                ..AgentRepo::new("firma/neu")
            }],
            default: None,
        }),
        ..Default::default()
    };
    let (text, c) = change("", &a).unwrap();
    assert!(text.contains("watch = false"), "{text}");
    assert!(!c.agent_repos[0].watch);
    assert!(c.github.is_none(), "stored settings do not widen the sync");
    assert_eq!(c.agent_repos[0].base_branch, "develop");
}

#[test]
fn choosing_the_watched_repos_unwatches_the_entries_that_left_the_list() {
    let start = "[github]\nrepos = [\"firma/app\"]\n[[agent.repos]]\nrepo = \"firma/web\"\n";
    let before = Config::parse(Some(start), |_| None).unwrap();
    assert_eq!(before.github.unwrap().repos, ["firma/app", "firma/web"]);
    let a = ConfigChange {
        github: Some(cockpit_core::api::GitHubInput {
            repos: vec!["firma/app".into()],
        }),
        ..Default::default()
    };
    let (text, c) = change(start, &a).unwrap();
    assert!(text.contains("watch = false"), "{text}");
    assert_eq!(c.github.unwrap().repos, ["firma/app"]);
    assert_eq!(c.agent_repos[0].repo, "firma/web");
}

#[test]
fn a_branch_pattern_is_written_only_when_it_is_not_the_default() {
    use cockpit_core::AgentRepo;
    use cockpit_core::api::AgentReposInput;
    let a = ConfigChange {
        agent_repos: Some(AgentReposInput {
            repos: vec![
                AgentRepo::new("firma/app"),
                AgentRepo {
                    branch_pattern: "{type}/{ticket}-{slug}".into(),
                    ..AgentRepo::new("firma/web")
                },
            ],
            default: None,
        }),
        ..Default::default()
    };
    let (text, c) = change("", &a).unwrap();
    assert_eq!(text.matches("branch_pattern").count(), 1, "{text}");
    assert!(text.contains("branch_pattern = \"{type}/{ticket}-{slug}\""));
    assert_eq!(c.agent_repos[0].branch_pattern, "claude/{ticket}-{slug}");
    assert_eq!(c.agent_repos[1].branch_pattern, "{type}/{ticket}-{slug}");
}

#[test]
fn saving_an_invalid_branch_pattern_is_refused_and_writes_nothing() {
    use cockpit_core::AgentRepo;
    use cockpit_core::api::AgentReposInput;
    for bad in ["main", "feature/{slug}", "{ticket}{slug}", "a b/{ticket}"] {
        let a = ConfigChange {
            agent_repos: Some(AgentReposInput {
                repos: vec![AgentRepo {
                    branch_pattern: bad.into(),
                    ..AgentRepo::new("firma/web")
                }],
                default: None,
            }),
            ..Default::default()
        };
        assert!(change("", &a).is_err(), "{bad}");
    }
}

#[test]
fn saving_an_empty_list_removes_both_places() {
    use cockpit_core::api::AgentReposInput;
    let a = ConfigChange {
        agent_repos: Some(AgentReposInput::default()),
        ..Default::default()
    };
    let (text, c) = change(
        "[github]\nrepos = [\"firma/app\"]\n[agent]\nrepo = \"firma/app\"\n",
        &a,
    )
    .unwrap();
    // Only the entries of the agent go; the watched repos of `[github]` stay.
    assert!(
        !text.contains("[agent]") && text.contains("firma/app"),
        "{text}"
    );
    assert!(c.agent_repos.is_empty());
    assert_eq!(c.github.unwrap().repos, ["firma/app"]);
}

#[test]
fn a_hand_made_old_list_and_the_wizard_see_one_list() {
    use cockpit_core::{AgentRepo, AgentRepos};
    let c = Config::parse(Some(OLD_GITHUB), |_| None).unwrap();
    let configured = AgentRepos {
        repos: c.agent_repos.clone(),
        default: c.agent_repo.clone(),
    };
    let all = configured.unified(&c.github.unwrap().repos);
    assert_eq!(
        all.repos,
        [AgentRepo::new("firma/app"), AgentRepo::new("firma/alt")]
    );
    assert_eq!(all.default.as_deref(), Some("firma/app"));
}

#[test]
fn removing_the_client_id_deletes_the_section_from_the_file() {
    let start = "[atlassian]\nclient_id = \"old\"\nsite = \"firma\"\n\n# my notes\n[github]\nrepos = [\"a/b\"]\n";
    let a = ConfigChange {
        atlassian: Some(ClientInput::default()),
        ..Default::default()
    };
    let (updated, c) = change(start, &a).unwrap();
    assert!(c.atlassian.is_none());
    assert!(
        !updated.contains("atlassian")
            && !updated.contains("client_id")
            && !updated.contains("firma"),
        "{updated}"
    );
    assert!(
        updated.contains("# my notes") && updated.contains("repos"),
        "{updated}"
    );
    // The file still reads at the next start (`deny_unknown_fields`).
    assert!(Config::parse(Some(&updated), |_| None).is_ok());
    // Removing it twice is no error.
    let (again, _) = change(&updated, &a).unwrap();
    assert_eq!(again, updated);
}

#[test]
fn changing_the_client_id_keeps_the_site_and_removing_the_site_works() {
    let start = "[atlassian]\nclient_id = \"old\"\nsite = \"firma\"\n";
    let new_id = ConfigChange {
        atlassian: Some(ClientInput {
            client_id: "new".into(),
        }),
        ..Default::default()
    };
    let (updated, c) = change(start, &new_id).unwrap();
    let a = c.atlassian.unwrap();
    assert_eq!(
        (a.client_id.as_str(), a.site.as_deref()),
        ("new", Some("firma"))
    );
    assert!(!updated.contains("old"), "{updated}");

    let no_site = ConfigChange {
        atlassian_site: Some(String::new()),
        ..Default::default()
    };
    let (updated, c) = change(&updated, &no_site).unwrap();
    assert_eq!(c.atlassian.unwrap().site, None);
    assert!(!updated.contains("site"), "{updated}");

    let site = ConfigChange {
        atlassian_site: Some(" Firma Eins ".into()),
        ..Default::default()
    };
    let (updated, c) = change(&updated, &site).unwrap();
    assert_eq!(c.atlassian.unwrap().site.as_deref(), Some("Firma Eins"));
    assert!(updated.contains("site = \"Firma Eins\""), "{updated}");
}

#[test]
fn a_site_without_a_client_id_is_refused() {
    let a = ConfigChange {
        atlassian_site: Some("firma".into()),
        ..Default::default()
    };
    assert!(change("", &a).is_err());
    // Removing a site that is not there is fine.
    let none = ConfigChange {
        atlassian_site: Some(String::new()),
        ..Default::default()
    };
    assert_eq!(change("", &none).unwrap().0, "");
}

#[test]
fn change_writes_the_chosen_sign_in_method_and_reads_it_back() {
    use cockpit_core::AuthChoice;
    let a = ConfigChange {
        github_auth: Some(AuthChoice::Token),
        jira_auth: Some(AuthChoice::OAuth),
        atlassian: Some(ClientInput {
            client_id: "7dNimh".into(),
        }),
        ..Default::default()
    };
    let (updated, c) = change("[github]\n# mine\nrepos = [\"acme/app\"]\n", &a).unwrap();
    assert!(updated.contains("auth = \"token\""), "{updated}");
    assert!(updated.contains("# mine"), "{updated}");
    assert_eq!(c.github_auth, Some(AuthChoice::Token));
    assert_eq!(c.jira_auth, Some(AuthChoice::OAuth));
    assert_eq!(c.github.unwrap().repos, vec!["acme/app"]);
    // Changing it again replaces the value; a file without the key reads as no choice.
    let b = ConfigChange {
        github_auth: Some(AuthChoice::OAuth),
        ..Default::default()
    };
    let (updated, c) = change(&updated, &b).unwrap();
    assert_eq!(c.github_auth, Some(AuthChoice::OAuth));
    assert_eq!(updated.matches("auth = \"oauth\"").count(), 2, "{updated}");
    let (_, c) = change("", &ConfigChange::default()).unwrap();
    assert_eq!((c.github_auth, c.jira_auth), (None, None));
}

#[test]
fn an_unknown_sign_in_method_in_the_file_is_an_error() {
    assert!(Config::parse(Some("[github]\nauth = \"magic\"\n"), |_| None).is_err());
}

#[test]
fn github_projects_are_written_in_place_and_checked() {
    use cockpit_core::TicketsFrom;
    use cockpit_core::api::GitHubProjectsInput;
    let input = |from: Option<TicketsFrom>, projects: Option<&[&str]>| ConfigChange {
        github_projects: Some(GitHubProjectsInput {
            tickets_from: from,
            projects: projects.map(|p| p.iter().map(|s| (*s).to_string()).collect()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (text, config) = change(
        WITH_COMMENTS,
        &input(
            Some(TicketsFrom::GitHubProjects),
            Some(&["acme/3", " mkeller/1 "]),
        ),
    )
    .unwrap();
    assert!(
        text.contains("# Diese Repos gleicht Cockpit ab"),
        "other comments stay"
    );
    assert!(
        text.contains("[github_projects]") && text.contains("tickets_from = \"github_projects\"")
    );
    let p = config.github_projects.unwrap();
    assert_eq!(p.tickets_from, TicketsFrom::GitHubProjects);
    assert_eq!(p.projects.len(), 2);
    // Only the choice changes: the projects stay.
    let (text2, config2) = change(&text, &input(Some(TicketsFrom::Both), None)).unwrap();
    assert_eq!(config2.github_projects.unwrap().projects.len(), 2);
    assert!(text2.contains("tickets_from = \"both\""));
    // An empty field name removes the choice; a malformed project is refused and nothing is written.
    let cleared = ConfigChange {
        github_projects: Some(GitHubProjectsInput {
            team_field: Some("  ".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(change(&text2, &cleared).is_ok());
    assert!(change(WITH_COMMENTS, &input(None, Some(&["not a project"]))).is_err());
}
