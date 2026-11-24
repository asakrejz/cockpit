use super::*;

fn repos() -> AgentRepos {
    AgentRepos {
        repos: vec![
            AgentRepo::new("acme/app"),
            AgentRepo {
                base_branch: "develop".into(),
                ..AgentRepo::new("acme/web")
            },
        ],
        default: Some("acme/app".into()),
    }
}

#[test]
fn every_repo_has_its_own_base_branch() {
    let r = repos();
    assert_eq!(r.base_branch("acme/web"), "develop");
    assert_eq!(r.base_branch("acme/app"), "main");
    assert_eq!(r.base_branch("acme/unknown"), "main");
}

#[test]
fn the_names_keep_the_order_of_the_list() {
    assert_eq!(repos().names(), ["acme/app", "acme/web"]);
}

#[test]
fn old_json_without_fields_reads() {
    let r: AgentRepos = serde_json::from_str("{}").unwrap();
    assert!(r.is_empty() && r.default.is_none());
    let one: AgentRepo = serde_json::from_str(r#"{"repo":"a/b"}"#).unwrap();
    assert_eq!(one, AgentRepo::new("a/b"));
}

#[test]
fn old_json_with_the_removed_project_list_reads_and_ignores_it() {
    let one: AgentRepo =
        serde_json::from_str(r#"{"repo":"a/b","base_branch":"dev","projects":["WEB","SHOP"]}"#)
            .unwrap();
    assert_eq!(
        one,
        AgentRepo {
            base_branch: "dev".into(),
            ..AgentRepo::new("a/b")
        }
    );
    // The list is not written any more.
    assert!(!serde_json::to_string(&one).unwrap().contains("projects"));
}
