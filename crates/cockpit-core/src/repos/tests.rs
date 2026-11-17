use super::*;

fn repos() -> AgentRepos {
    AgentRepos {
        repos: vec![
            AgentRepo::new("acme/app"),
            AgentRepo {
                base_branch: "develop".into(),
                projects: vec!["WEB".into(), "SHOP".into()],
                ..AgentRepo::new("acme/web")
            },
        ],
        default: Some("acme/app".into()),
    }
}

#[test]
fn a_rule_wins_over_the_default() {
    let r = repos();
    assert_eq!(r.choose(Some("WEB")), RepoChoice::Rule("acme/web".into()));
    assert_eq!(r.choose(Some("shop")), RepoChoice::Rule("acme/web".into()));
    assert_eq!(r.base_branch("acme/web"), "develop");
    assert_eq!(r.base_branch("acme/app"), "main");
}

#[test]
fn without_a_matching_rule_the_default_applies() {
    let r = repos();
    assert_eq!(
        r.choose(Some("BAU")),
        RepoChoice::Default("acme/app".into())
    );
    assert_eq!(r.choose(None), RepoChoice::Default("acme/app".into()));
}

#[test]
fn without_rule_and_default_nothing_is_chosen() {
    let r = AgentRepos {
        default: None,
        ..repos()
    };
    assert_eq!(r.choose(Some("BAU")), RepoChoice::None);
    assert_eq!(r.choose(Some("BAU")).repo(), None);
    assert_eq!(r.choose(Some("WEB")).repo(), Some("acme/web"));
}

#[test]
fn only_configured_repos_are_allowed() {
    let r = repos();
    assert!(r.allows("acme/web"));
    assert!(!r.allows("evil/other"));
}

#[test]
fn old_json_without_fields_reads() {
    let r: AgentRepos = serde_json::from_str("{}").unwrap();
    assert!(r.is_empty() && r.default.is_none());
    let one: AgentRepo = serde_json::from_str(r#"{"repo":"a/b"}"#).unwrap();
    assert_eq!(one, AgentRepo::new("a/b"));
}

#[test]
fn the_unified_list_adds_repos_that_only_the_older_section_names() {
    let r = repos();
    let all = r.unified(&["acme/old".into(), "acme/web".into()]);
    assert_eq!(all.names(), ["acme/app", "acme/web", "acme/old"]);
    // What is configured stays untouched: base branch, projects and default.
    assert_eq!(all.base_branch("acme/web"), "develop");
    assert_eq!(all.default.as_deref(), Some("acme/app"));
    assert_eq!(all.base_branch("acme/old"), "main");
    assert!(all.find("acme/old").unwrap().projects.is_empty());
}

#[test]
fn an_empty_configuration_takes_the_older_list_as_it_is() {
    let all = AgentRepos::default().unified(&["a/b".into(), "c/d".into()]);
    assert_eq!(all.names(), ["a/b", "c/d"]);
    assert_eq!(all.default, None);
    assert!(AgentRepos::default().unified(&[]).is_empty());
}

#[test]
fn a_project_routes_to_its_repo_in_the_unified_list() {
    let all = repos().unified(&["acme/old".into()]);
    assert_eq!(all.choose(Some("web")), RepoChoice::Rule("acme/web".into()));
    assert_eq!(
        all.choose(Some("X")),
        RepoChoice::Default("acme/app".into())
    );
    assert!(all.allows("acme/old") && !all.allows("acme/zzz"));
}
