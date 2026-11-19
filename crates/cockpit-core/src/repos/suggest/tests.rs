use super::*;
use crate::AgentRepo;

fn used(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(p, r)| (p.to_string(), r.to_string()))
        .collect()
}

fn names(s: &[RepoSuggestion]) -> Vec<&str> {
    s.iter().map(|s| s.repo.as_str()).collect()
}

#[test]
fn previous_runs_of_the_same_project_come_first_most_used_first() {
    let used = used(&[
        ("WEB", "acme/web"),
        ("WEB", "acme/api"),
        ("BAU", "acme/other"),
        ("WEB", "acme/api"),
        ("WEB", "acme/web"),
        ("WEB", "acme/api"),
    ]);
    let rules = AgentRepos::default();
    let s = suggest_repos(&SuggestInput {
        project: Some("web"),
        used: &used,
        rules: Some(&rules),
        ..Default::default()
    });
    assert_eq!(names(&s), ["acme/api", "acme/web"]);
    assert_eq!(
        s[0].reason,
        SuggestReason::PreviousRun {
            project: "WEB".into(),
            runs: 3
        }
    );
}

#[test]
fn equal_use_counts_prefer_the_latest_run() {
    let used = used(&[("WEB", "acme/a"), ("WEB", "acme/b")]);
    let rules = AgentRepos::default();
    let s = suggest_repos(&SuggestInput {
        project: Some("WEB"),
        used: &used,
        rules: Some(&rules),
        ..Default::default()
    });
    assert_eq!(names(&s), ["acme/b", "acme/a"]);
}

#[test]
fn the_reasons_keep_their_order_and_every_repo_appears_once() {
    let used = used(&[("WEB", "acme/web")]);
    let rules = AgentRepos {
        repos: vec![
            AgentRepo {
                projects: vec!["WEB".into()],
                ..AgentRepo::new("acme/rule")
            },
            AgentRepo::new("acme/app"),
        ],
        default: Some("acme/app".into()),
    };
    let linked = vec!["acme/linked".to_string(), "acme/web".to_string()];
    let visible = vec!["acme/text".to_string()];
    let s = suggest_repos(&SuggestInput {
        project: Some("WEB"),
        used: &used,
        linked: &linked,
        text: "Fix it in acme/text and https://github.com/acme/addr/issues/1",
        rules: Some(&rules),
        visible: Some(&visible),
    });
    assert_eq!(
        names(&s),
        [
            "acme/web",
            "acme/linked",
            "acme/text",
            "acme/addr",
            "acme/rule",
            "acme/app"
        ]
    );
    assert_eq!(
        s[0].reason.clone(),
        SuggestReason::PreviousRun {
            project: "WEB".into(),
            runs: 1
        }
    );
    assert_eq!(s[1].reason, SuggestReason::LinkedPr);
    assert_eq!(s[2].reason, SuggestReason::Mentioned);
    assert_eq!(
        s[4].reason,
        SuggestReason::Rule {
            project: "WEB".into()
        }
    );
    assert_eq!(s[5].reason, SuggestReason::Default);
}

#[test]
fn a_bare_name_in_the_text_needs_a_visible_repo() {
    let rules = AgentRepos::default();
    let text = "Look at docs/readme and acme/app";
    let none = suggest_repos(&SuggestInput {
        text,
        rules: Some(&rules),
        ..Default::default()
    });
    assert!(none.is_empty());
    let visible = vec!["Acme/App".to_string()];
    let s = suggest_repos(&SuggestInput {
        text,
        rules: Some(&rules),
        visible: Some(&visible),
        ..Default::default()
    });
    assert_eq!(names(&s), ["acme/app"]);
}

#[test]
fn without_a_project_only_link_text_and_default_count() {
    let used = used(&[("WEB", "acme/web")]);
    let rules = AgentRepos {
        repos: vec![],
        default: Some("acme/app".into()),
    };
    let s = suggest_repos(&SuggestInput {
        project: None,
        used: &used,
        rules: Some(&rules),
        ..Default::default()
    });
    assert_eq!(names(&s), ["acme/app"]);
}

#[test]
fn invalid_names_are_dropped() {
    let rules = AgentRepos::default();
    let linked = vec!["nonsense".to_string(), "a b/c".to_string()];
    let s = suggest_repos(&SuggestInput {
        linked: &linked,
        rules: Some(&rules),
        ..Default::default()
    });
    assert!(s.is_empty());
}

#[test]
fn texts_yield_repos_and_addresses() {
    let found = repos_in_text(
        "Repo: acme/app. Also (acme/web.git), [acme/shop|https://github.com/acme/shop/pull/4] \
         and https://gitlab.com/acme/nope plus a/b/c and /usr/bin",
    );
    let list: Vec<(&str, bool)> = found
        .iter()
        .map(|m| (m.repo.as_str(), m.from_address))
        .collect();
    assert_eq!(
        list,
        [
            ("acme/app", false),
            ("acme/web", false),
            ("acme/shop", true)
        ]
    );
}

#[test]
fn an_address_upgrades_an_earlier_bare_mention() {
    let found = repos_in_text("acme/app https://github.com/acme/app/tree/main");
    assert_eq!(found.len(), 1);
    assert!(found[0].from_address);
}
