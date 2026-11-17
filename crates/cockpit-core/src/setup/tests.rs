use super::*;
use crate::api::KeyResponse;

#[test]
fn steps_walk_forward_and_back() {
    assert_eq!(Step::Container.previous(), None);
    assert_eq!(Step::Container.next(), Some(Step::Jira));
    assert_eq!(Step::Done.next(), None);
    assert_eq!(Step::Done.previous(), Some(Step::Live));
    assert_eq!(Step::Live.previous(), Some(Step::Model));
    for s in Step::ALL {
        assert_eq!(Step::from_id(s.id()), Some(s));
    }
    assert_eq!(Step::from_id("unsinn"), None);
}

#[test]
fn first_start_is_detected() {
    let e = Setup::default();
    let from = ServiceStatus::default();
    let l = EndpointStatus::default();
    assert!(nothing_set_up(&e, &from, &from, &l));
    let with_id = Setup {
        github_client_id: Some("Iv1".into()),
        ..Default::default()
    };
    assert!(!nothing_set_up(&with_id, &from, &from, &l));
    let an = ServiceStatus {
        connected: true,
        ..Default::default()
    };
    assert!(!nothing_set_up(&e, &an, &from, &l));
    let llm = EndpointStatus {
        key: true,
        ..Default::default()
    };
    assert!(!nothing_set_up(&e, &from, &from, &llm));
}

#[test]
fn keys_are_trimmed_and_checked_without_echo() {
    assert_eq!(key_verify("  test-wert-123\n").unwrap(), "test-wert-123");
    for bad in ["", "   ", "a\nb", "a\u{0}b"] {
        let error = key_verify(bad).unwrap_err();
        assert!(!error.contains('\u{0}'));
    }
    let long = "x".repeat(MAX_KEYS + 1);
    assert!(key_verify(&long).is_err());
    let msg = key_verify("geheim\nzwei").unwrap_err();
    assert!(!msg.contains("geheim"), "{msg}");
}

#[test]
fn only_known_key_names() {
    for n in [
        "github-oauth",
        "atlassian-oauth",
        "agent-key",
        "litellm",
        "jira",
        "github",
    ] {
        assert!(key_name_allowed(n));
    }
    for n in ["", "github-oauth-token", "../x", "Jira", "atlassian"] {
        assert!(!key_name_allowed(n), "{n}");
    }
}

#[test]
fn jira_site_in_all_spellings() {
    for ein in [
        "firma",
        "Firma.atlassian.net",
        "https://firma.atlassian.net/",
        "https://firma.atlassian.net/jira/software/projects/BAU/boards/1",
        "  firma.atlassian.net  ",
    ] {
        assert_eq!(
            jira_site_read(ein).unwrap(),
            "https://firma.atlassian.net",
            "{ein}"
        );
    }
    assert_eq!(jira_site_read("").unwrap(), "");
    // Only the own machine works without https.
    assert_eq!(
        jira_site_read("http://127.0.0.1:9911/x").unwrap(),
        "http://127.0.0.1:9911"
    );
    assert!(jira_site_read("http://127.0.0.1:").is_err());
    assert!(jira_site_read("http://127.0.0.1.evil.example").is_err());
    assert_eq!(
        jira_site_read("jira.firma.example").unwrap(),
        "https://jira.firma.example"
    );
    for bad in [
        "http://firma.atlassian.net",
        "ftp://x",
        "fir ma",
        "https://",
        "fi_rma",
    ] {
        assert!(jira_site_read(bad).is_err(), "{bad}");
    }
}

#[test]
fn projects_are_uppercased_and_deduplicated() {
    assert_eq!(
        projects_read("bau, web;BAU\nops"),
        vec!["BAU", "WEB", "OPS"]
    );
    assert!(projects_read("  ").is_empty());
}

#[test]
fn repos_from_names_and_urls() {
    assert_eq!(
        repos_read(
            "firma/app, https://github.com/firma/web.git\ngithub.com/firma/api/pull/3 firma/app"
        )
        .unwrap(),
        vec!["firma/app", "firma/web", "firma/api"]
    );
    assert!(repos_read("").unwrap().is_empty());
    for bad in ["app", "firma/", "/app", "fi rma/app x", "firma/ap$p"] {
        assert!(repos_read(bad).is_err(), "{bad}");
    }
}

#[test]
fn endpoint_url_gets_a_scheme() {
    assert_eq!(
        endpoint_url_read("localhost:4000/").unwrap(),
        "http://localhost:4000"
    );
    assert_eq!(
        endpoint_url_read(" https://llm.firma.example ").unwrap(),
        "https://llm.firma.example"
    );
    assert_eq!(endpoint_url_read("").unwrap(), "");
    for bad in ["ftp://x", "http://", "a b", "https:///"] {
        assert!(endpoint_url_read(bad).is_err(), "{bad}");
    }
}

#[test]
fn client_ids_have_no_spaces() {
    assert_eq!(client_id_read(" Iv23li ").unwrap(), "Iv23li");
    assert!(client_id_read("Iv23 li").is_err());
    assert_eq!(client_id_read("").unwrap(), "");
}

fn oauth(connected: bool) -> ServiceStatus {
    ServiceStatus {
        connected,
        kind: if connected {
            AuthKind::OAuth
        } else {
            AuthKind::None
        },
        ..Default::default()
    }
}

#[test]
fn summary_explains_in_plain_words() {
    let empty = summary(&Snapshot::default());
    assert_eq!(empty.len(), 4);
    assert!(empty.iter().all(|z| z.tone == Tone::Open), "{empty:?}");

    // Client ID there, secret missing: a problem that names the step.
    let mut s = Snapshot::default();
    s.setup.github_client_id = Some("Iv1".into());
    let z = summary(&s);
    assert_eq!(z[2].tone, Tone::Problem);
    assert_eq!(z[2].step, Step::GitHub);
    assert!(z[2].text.contains("Client-Secret"));

    // Both there, only signing in missing: open with instructions.
    s.setup.keys = vec![KeyResponse {
        name: "github-oauth".into(),
        set: true,
    }];
    let z = summary(&s);
    assert_eq!(z[2].tone, Tone::Open);
    assert!(z[2].text.contains("Mit GitHub anmelden"));

    // Signed in, but without repos: problem.
    s.github = oauth(true);
    assert_eq!(summary(&s)[2].tone, Tone::Problem);
    s.setup.github_repos = vec!["firma/app".into()];
    assert_eq!(summary(&s)[2].tone, Tone::Good);
}

#[test]
fn container_and_model_lines() {
    let mut s = Snapshot::default();
    s.container.runtime = Some("docker".into());
    let z = summary(&s);
    assert_eq!(z[0].tone, Tone::Problem);
    s.container.engine_running = true;
    assert_eq!(summary(&s)[0].tone, Tone::Good);

    s.endpoint.configured = true;
    assert_eq!(summary(&s)[3].tone, Tone::Problem);
    s.endpoint.key = true;
    assert!(summary(&s)[3].text.contains("Modellname"));
    s.endpoint.model = Some("m".into());
    assert_eq!(summary(&s)[3].tone, Tone::Open);
    s.check = Some(EndpointCheck {
        configured: true,
        reachable: true,
        key_ok: true,
        model_present: true,
        hint: None,
    });
    assert_eq!(summary(&s)[3].tone, Tone::Good);
}

#[test]
fn test_result_tells_where_it_failed() {
    let mut p = EndpointCheck {
        configured: true,
        ..Default::default()
    };
    assert!(check_plain_text(&p).1.contains("antwortet nicht"));
    p.reachable = true;
    assert!(check_plain_text(&p).1.contains("Key"));
    p.key_ok = true;
    assert!(check_plain_text(&p).1.contains("Modellnamen"));
    p.model_present = true;
    assert_eq!(check_plain_text(&p).0, Tone::Good);
    assert_eq!(check_plain_text(&EndpointCheck::default()).0, Tone::Open);
}

#[test]
fn summary_asks_for_the_image_when_the_real_agent_needs_it() {
    let mut s = Snapshot::default();
    s.container.runtime = Some("docker".into());
    s.container.engine_running = true;
    // Simulated or unknown: green as before.
    assert_eq!(summary(&s)[0].tone, Tone::Good);
    s.agent.mode = "real".into();
    let z = &summary(&s)[0];
    assert_eq!(z.tone, Tone::Open);
    assert!(z.text.contains("Image bauen"));
    s.agent.image = true;
    assert_eq!(summary(&s)[0].tone, Tone::Good);
}

#[test]
fn endpoint_kinds_roundtrip_and_follow_the_url() {
    for a in EndpointKind::ALL {
        assert_eq!(EndpointKind::from_id(a.id()), Some(a));
    }
    assert_eq!(EndpointKind::from_url(""), EndpointKind::Direct);
    assert_eq!(EndpointKind::from_url("http://x:1"), EndpointKind::Proxy);
    assert!(!EndpointKind::Direct.needs_address());
    assert!(EndpointKind::Compatible.needs_address());
    assert!(
        !EndpointKind::Direct
            .explanation(Provider::ClaudeCode)
            .contains("LiteLLM")
    );
    assert!(
        EndpointKind::Direct
            .explanation(Provider::Codex)
            .contains("OpenAI")
    );
    assert_eq!(
        EndpointKind::Direct.key_label(Provider::Codex),
        "OpenAI-API-Key"
    );
}

#[test]
fn model_choice_keeps_an_unknown_saved_model_visible() {
    let list = vec!["claude-a".to_string(), "gpt-x".to_string()];
    let o = model_selection(&list, "claude-a");
    assert_eq!(o.len(), 2);
    assert!(o.iter().all(|m| !m.unknown));
    assert_eq!(model_hint(&list, "claude-a"), None);

    let o = model_selection(&list, " alt-modell ");
    assert_eq!(o.len(), 3);
    assert_eq!((o[0].id.as_str(), o[0].unknown), ("alt-modell", true));
    assert!(
        model_hint(&list, "alt-modell")
            .unwrap()
            .contains("alt-modell")
    );

    // Empty list or nothing stored: no hint, no extra row.
    assert!(model_selection(&list, "").len() == 2);
    assert_eq!(model_hint(&[], "x"), None);
    assert_eq!(model_hint(&list, ""), None);
}

#[test]
fn hand_typed_model_names_are_checked() {
    assert_eq!(
        model_name_read("  claude-sonnet-5-5 ").unwrap(),
        "claude-sonnet-5-5"
    );
    assert_eq!(model_name_read("").unwrap(), "");
    assert_eq!(model_name_read("a/b:c.1").unwrap(), "a/b:c.1");
    for bad in ["a b", "a\nb", &"x".repeat(201)] {
        assert!(model_name_read(bad).is_err());
    }
}

#[test]
fn the_summary_names_the_chosen_tool() {
    let mut s = Snapshot::default();
    assert!(!summary(&s)[3].text.contains("Codex"));
    s.endpoint.provider = Provider::Codex;
    s.endpoint.configured = true;
    let z = &summary(&s)[3];
    assert_eq!(z.tone, Tone::Problem);
    assert!(z.text.starts_with("Codex: "), "{}", z.text);
}

#[test]
fn jira_line_names_the_kind_of_login() {
    let token = ServiceStatus {
        connected: true,
        kind: AuthKind::Token,
        ..Default::default()
    };
    let mut s = Snapshot {
        jira: token,
        ..Default::default()
    };
    let z = &summary(&s)[1];
    assert_eq!((z.tone, z.name), (Tone::Good, "Jira"));
    assert!(z.text.contains("mit API-Token"), "{}", z.text);
    s.jira = oauth(true);
    let z = &summary(&s)[1];
    assert_eq!(z.tone, Tone::Good);
    assert!(z.text.contains("mit OAuth"), "{}", z.text);
}

#[test]
fn jira_line_says_what_the_token_way_lacks() {
    let mut s = Snapshot::default();
    let z = &summary(&s)[1];
    assert_eq!(z.tone, Tone::Open);
    assert!(z.text.contains("API-Token"));
    s.setup.jira_site = "https://firma.atlassian.net".into();
    let z = &summary(&s)[1];
    assert!(
        z.text.contains("E-Mail-Adresse und der Token"),
        "{}",
        z.text
    );
    s.setup.jira_email = "a@b.de".into();
    let z = &summary(&s)[1];
    assert!(z.text.ends_with("der Token."), "{}", z.text);
    // The way via the app is kept.
    let mut o = Snapshot::default();
    o.setup.atlassian_client_id = Some("id".into());
    let z = &summary(&o)[1];
    assert_eq!(z.tone, Tone::Problem);
    assert!(z.text.contains("Client-Secret"));
}

#[test]
fn first_start_counts_token_and_oauth_alike() {
    let from = ServiceStatus::default();
    let l = EndpointStatus::default();
    let e = Setup::default();
    for kind in [AuthKind::Token, AuthKind::OAuth] {
        let an = ServiceStatus {
            connected: true,
            kind,
            ..Default::default()
        };
        assert!(!nothing_set_up(&e, &an, &from, &l));
    }
}

#[test]
fn jira_test_texts_never_mention_a_token_value() {
    for u in [
        JiraCause::Ok,
        JiraCause::Details,
        JiraCause::Address,
        JiraCause::Token,
        JiraCause::Permission,
        JiraCause::Reachable,
        JiraCause::Other,
    ] {
        let t = jira_check_text(u, Some("Ada"));
        assert!(!t.is_empty());
        assert!(!t.contains("test-wert"));
    }
    assert!(jira_check_text(JiraCause::Ok, Some("Ada")).contains("kennt dich: Ada"));
    assert!(jira_check_text(JiraCause::Ok, None).ends_with("kennt dich."));
}

#[test]
fn only_tokens_can_be_deleted() {
    assert!(key_deletable("jira") && key_deletable("github"));
    for n in ["litellm", "atlassian-oauth", "github-oauth", "", "Jira"] {
        assert!(!key_deletable(n), "{n}");
    }
}

fn row(repo: &str, base: &str, projects: &str) -> (String, String, String) {
    (repo.into(), base.into(), projects.into())
}

#[test]
fn agent_repos_come_from_rows() {
    let a = agent_repos_read(
        &[
            row("firma/app", "", ""),
            row("https://github.com/firma/web.git", "develop", "web, shop"),
            row("", "", ""),
        ],
        "firma/app",
    )
    .unwrap();
    assert_eq!(a.repos.len(), 2);
    assert_eq!(a.repos[0].base_branch, "main");
    assert_eq!(a.repos[1].repo, "firma/web");
    assert_eq!(a.repos[1].projects, ["WEB", "SHOP"]);
    assert_eq!(a.default.as_deref(), Some("firma/app"));
    // No default is fine; an empty form clears everything.
    assert_eq!(
        agent_repos_read(&[row("a/b", "", "")], "").unwrap().default,
        None
    );
    assert_eq!(agent_repos_read(&[], "").unwrap(), Default::default());
}

#[test]
fn agent_repos_rows_with_mistakes_say_what_is_wrong() {
    for (rows, default, part) in [
        (vec![row("firma", "", "")], "", "Repo"),
        (vec![row("a/b", "", ""), row("a/b", "", "")], "", "doppelt"),
        (vec![row("a/b", "-x", "")], "", "Basis-Branch"),
        (vec![row("a/b", "", "1X")], "", "Projektschlüssel"),
        (
            vec![row("a/b", "", "WEB"), row("a/c", "", "web")],
            "",
            "WEB",
        ),
        (vec![row("a/b", "", "")], "a/c", "Standard-Repo"),
    ] {
        let e = agent_repos_read(&rows, default).unwrap_err();
        assert!(e.contains(part), "{e}");
    }
}

#[test]
fn public_url_is_checked_and_normalized() {
    assert_eq!(public_url_read(""), Ok(String::new()));
    assert_eq!(public_url_read("  "), Ok(String::new()));
    assert_eq!(
        public_url_read(" https://cockpit.example/ "),
        Ok("https://cockpit.example".into())
    );
    assert_eq!(
        public_url_read("https://a.example:8443/tunnel"),
        Ok("https://a.example:8443/tunnel".into())
    );
    for bad in [
        "http://cockpit.example",
        "cockpit.example",
        "https://",
        "https://a b.example",
        "https://u@cockpit.example",
        "https://cockpit.example/?x=1",
        "https://cockpit.example/#a",
    ] {
        assert!(public_url_read(bad).is_err(), "{bad}");
    }
}

#[test]
fn webhook_texts() {
    assert_eq!(
        webhook_target(Some("https://cockpit.example"), "/api/webhooks/github"),
        "https://cockpit.example/api/webhooks/github"
    );
    assert_eq!(
        webhook_target(None, "/api/webhooks/jira"),
        "<öffentliche Adresse>/api/webhooks/jira"
    );
    assert!(webhook_received_text(None, None, 5).starts_with("Noch nichts"));
    assert_eq!(
        webhook_received_text(Some(0), Some("check_run"), 180_000),
        "Zuletzt empfangen vor 3 Min. (check_run)."
    );
}

#[test]
fn webhook_keys_are_allowed_names() {
    assert!(key_name_allowed("github-webhook") && key_name_allowed("jira-webhook"));
    assert!(!key_deletable("github-webhook"));
}

#[test]
fn github_texts_never_name_a_token_and_the_ok_names_the_user() {
    use crate::api::GitHubCause;
    for c in [
        GitHubCause::Ok,
        GitHubCause::NotConnected,
        GitHubCause::Token,
        GitHubCause::Permission,
        GitHubCause::RateLimit,
        GitHubCause::Network,
        GitHubCause::Other,
    ] {
        let t = github_check_text(c, Some("ada"));
        assert!(!t.is_empty() && !t.contains("ghp_"), "{c:?}");
    }
    assert!(github_check_text(GitHubCause::Ok, Some("ada")).contains("als ada"));
    assert_eq!(
        github_check_text(GitHubCause::Ok, None),
        "GitHub antwortet."
    );
}

#[test]
fn the_repo_search_needs_every_word_and_ignores_case() {
    assert!(repo_matches("Firma/Web-Shop", ""));
    assert!(repo_matches("Firma/Web-Shop", "web"));
    assert!(repo_matches("Firma/Web-Shop", "firma SHOP"));
    assert!(!repo_matches("Firma/Web-Shop", "firma api"));
}

#[test]
fn the_repo_summary_says_found_selected_and_the_search() {
    assert_eq!(
        repo_summary(23, 23, 2, false),
        "23 Repos gefunden, 2 ausgewählt."
    );
    assert_eq!(
        repo_summary(1, 1, 0, false),
        "1 Repo gefunden, keins ausgewählt."
    );
    assert_eq!(
        repo_summary(23, 4, 2, false),
        "23 Repos gefunden, 2 ausgewählt. Die Suche zeigt 4."
    );
    assert!(repo_summary(500, 500, 1, true).contains("noch mehr"));
}
