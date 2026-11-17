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
