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
        repos: Some(&rules),
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
        repos: Some(&rules),
        ..Default::default()
    });
    assert_eq!(names(&s), ["acme/b", "acme/a"]);
}

#[test]
fn the_reasons_keep_their_order_and_every_repo_appears_once() {
    let used = used(&[("WEB", "acme/web")]);
    let rules = AgentRepos {
        repos: vec![AgentRepo::new("acme/app")],
        default: Some("acme/app".into()),
    };
    let linked = vec!["acme/linked".to_string(), "acme/web".to_string()];
    let visible = vec!["acme/text".to_string()];
    let s = suggest_repos(&SuggestInput {
        issue_repo: None,
        project: Some("WEB"),
        used: &used,
        linked: &linked,
        text: "Fix it in acme/text and https://github.com/acme/addr/issues/1",
        repos: Some(&rules),
        visible: Some(&visible),
    });
    assert_eq!(
        names(&s),
        [
            "acme/web",
            "acme/linked",
            "acme/text",
            "acme/addr",
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
    assert_eq!(s[4].reason, SuggestReason::Default);
}

#[test]
fn a_bare_name_in_the_text_needs_a_visible_repo() {
    let rules = AgentRepos::default();
    let text = "Look at docs/readme and acme/app";
    let none = suggest_repos(&SuggestInput {
        text,
        repos: Some(&rules),
        ..Default::default()
    });
    assert!(none.is_empty());
    let visible = vec!["Acme/App".to_string()];
    let s = suggest_repos(&SuggestInput {
        text,
        repos: Some(&rules),
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
        repos: Some(&rules),
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
        repos: Some(&rules),
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

#[test]
fn the_picker_lists_suggestions_first_then_the_rest_and_the_shared_search_narrows_both() {
    let s = vec![
        RepoSuggestion {
            repo: "acme/web".into(),
            reason: SuggestReason::LinkedPr,
        },
        RepoSuggestion {
            repo: "acme/shop".into(),
            reason: SuggestReason::Mentioned,
        },
    ];
    let all: Vec<String> = ["acme/zed", "acme/api", "Acme/Web", "acme/shop"]
        .iter()
        .map(|r| r.to_string())
        .collect();
    let names = |rows: &[PickerRow]| rows.iter().map(|r| r.repo.clone()).collect::<Vec<_>>();
    let rows = picker_rows(&s, &all);
    assert_eq!(
        names(&rows),
        ["acme/web", "acme/shop", "acme/api", "acme/zed"]
    );
    assert!(rows[2].reason.is_none());
    // The search of every repo list narrows suggestions and the rest alike.
    let found = crate::setup::repo_page(&rows, |r| r.repo.as_str(), "sho");
    assert_eq!(names(&found.shown), ["acme/shop"]);
    // Without the list from GitHub only the suggestions are there.
    assert_eq!(names(&picker_rows(&s, &[])), ["acme/web", "acme/shop"]);
}

#[test]
fn the_repo_of_an_issue_comes_first_before_any_other_reason() {
    let used = vec![("WEB".to_string(), "acme/web".to_string())];
    let linked = vec!["acme/linked".to_string()];
    let s = suggest_repos(&SuggestInput {
        issue_repo: Some("acme/app"),
        project: Some("WEB"),
        used: &used,
        linked: &linked,
        ..Default::default()
    });
    assert_eq!(names(&s), ["acme/app", "acme/web", "acme/linked"]);
    assert_eq!(s[0].reason, SuggestReason::Issue);
    // The same repo from another reason is not listed twice.
    let again = suggest_repos(&SuggestInput {
        issue_repo: Some("acme/linked"),
        linked: &linked,
        ..Default::default()
    });
    assert_eq!(names(&again), ["acme/linked"]);
    assert_eq!(again[0].reason, SuggestReason::Issue);
}

#[test]
fn the_start_dialog_suggests_from_history_links_and_text_with_the_default_last() {
    // Jira projects are not tied to repos: earlier runs, linked pull requests and mentions
    // come first, and the default repo comes last.
    let used = vec![("WEB".to_string(), "acme/web".to_string())];
    let linked = vec!["acme/api".to_string()];
    let rules = AgentRepos {
        repos: vec![AgentRepo::new("acme/app")],
        default: Some("acme/app".into()),
    };
    let s = suggest_repos(&SuggestInput {
        project: Some("WEB"),
        used: &used,
        linked: &linked,
        text: "see https://github.com/acme/docs/pull/3",
        repos: Some(&rules),
        ..Default::default()
    });
    assert_eq!(names(&s), ["acme/web", "acme/api", "acme/docs", "acme/app"]);
    assert_eq!(s[3].reason, SuggestReason::Default);
}

#[test]
fn repos_without_a_default_suggest_nothing_by_themselves() {
    let rules = AgentRepos {
        repos: vec![AgentRepo::new("acme/web"), AgentRepo::new("acme/app")],
        default: None,
    };
    let s = suggest_repos(&SuggestInput {
        project: Some("web"),
        repos: Some(&rules),
        ..Default::default()
    });
    assert!(s.is_empty());
}
