//! One run in several repos: per-repo results and pull requests.

use super::*;
use crate::agent::{PullRequestRef, RepoResult, RunRepoState};

fn options() -> AgentOptions {
    AgentOptions {
        plan_first: false,
        repos: vec!["acme/app".into(), "acme/web".into(), "acme/api".into()],
        targets: vec![crate::agent::RepoTarget {
            repo: "acme/web".into(),
            base_branch: "develop".into(),
            branch_pattern: "feature/{ticket}".into(),
            pr_language: cockpit_i18n::Lang::De,
        }],
        ..Default::default()
    }
}

fn result(repo: &str, state: RunRepoState) -> RepoResult {
    RepoResult {
        repo: repo.into(),
        state,
        files: vec!["src/a.rs".into()],
        reason: if state == RunRepoState::Refused {
            "forbidden path".into()
        } else {
            String::new()
        },
    }
}

/// BAU-142 worked in three repos: `app` pushed, `web` refused, `api` unchanged.
fn done() -> Workspace {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: options(),
        },
        1,
    );
    report(
        &mut w,
        "BAU-142",
        AgentReport::Started {
            detail: "docker".into(),
        },
    )
    .unwrap();
    report(
        &mut w,
        "BAU-142",
        AgentReport::Repos {
            repos: vec![
                result("acme/app", RunRepoState::Pushed),
                result("acme/web", RunRepoState::Refused),
                result("acme/api", RunRepoState::Unchanged),
            ],
        },
    )
    .unwrap();
    report(
        &mut w,
        "BAU-142",
        AgentReport::Done {
            summary: "Done.".into(),
        },
    )
    .unwrap();
    w
}

fn create(w: &mut Workspace, repo: &str, number: u32) -> Result<Applied, ActionError> {
    w.apply(
        Action::CreatePullRequest {
            run: "BAU-142".into(),
            title: "BAU-142: Toggle".into(),
            body: "Text".into(),
            pull: Some(PullRequestRef {
                number,
                url: format!("https://github.example/{repo}/pull/{number}"),
                existed: false,
            }),
            repo: repo.into(),
        },
        Actor::User,
        T,
    )
}

#[test]
fn a_start_with_several_repos_keeps_each_ones_settings() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: options(),
        },
        1,
    );
    let run = w.run("BAU-142").unwrap();
    assert_eq!(run.repo, "acme/app", "the first repo is the primary one");
    assert_eq!(run.repos.len(), 3);
    assert_eq!(run.repos[1].base_branch, "develop");
    assert_eq!(run.branch_in("acme/web"), "feature/BAU-142");
    assert_eq!(run.branch_in("acme/app"), run.branch());
    assert!(run.repos.iter().all(|r| r.state == RunRepoState::Pending));
}

#[test]
fn only_a_pushed_repo_gets_a_pull_request_and_each_only_one() {
    let mut w = done();
    for repo in ["acme/web", "acme/api"] {
        assert_eq!(
            create(&mut w, repo, 5),
            Err(ActionError::PullRequestNotReady),
            "{repo}"
        );
    }
    assert_eq!(
        create(&mut w, "acme/other", 5),
        Err(ActionError::PullRequestNotReady)
    );
    create(&mut w, "acme/app", 5).unwrap();
    assert_eq!(
        create(&mut w, "acme/app", 6),
        Err(ActionError::PullRequestExists(5))
    );
    let run = w.run("BAU-142").unwrap();
    assert_eq!(run.repos[0].state, RunRepoState::PrCreated);
    assert_eq!(run.repos[1].state, RunRepoState::Refused);
    assert_eq!(run.pr_key().unwrap().number, 5);
}

#[test]
fn the_pull_request_text_links_the_sibling_pull_requests() {
    let mut w = ws();
    let mut o = options();
    o.repos.truncate(2);
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: o,
        },
        1,
    );
    report(
        &mut w,
        "BAU-142",
        AgentReport::Started {
            detail: "docker".into(),
        },
    )
    .unwrap();
    report(
        &mut w,
        "BAU-142",
        AgentReport::Repos {
            repos: vec![
                result("acme/app", RunRepoState::Pushed),
                result("acme/web", RunRepoState::Pushed),
            ],
        },
    )
    .unwrap();
    report(
        &mut w,
        "BAU-142",
        AgentReport::Done {
            summary: "Done.".into(),
        },
    )
    .unwrap();
    // Before the sibling has a pull request, its branch is named.
    let first = w.pull_request_draft("BAU-142", "acme/app").unwrap();
    assert!(
        first.body.contains("acme/web: `feature/BAU-142`"),
        "{}",
        first.body
    );
    assert_eq!(first.head, w.run("BAU-142").unwrap().branch());
    create(&mut w, "acme/app", 5).unwrap();
    // The second one links the first, in its own language and against its own base.
    let second = w.pull_request_draft("BAU-142", "acme/web").unwrap();
    assert!(
        second
            .body
            .contains("acme/app#5: https://github.example/acme/app/pull/5"),
        "{}",
        second.body
    );
    assert!(second.body.contains("Dieselbe Änderung in anderen Repos"));
    assert_eq!(second.base, "develop");
    assert_eq!(second.head, "feature/BAU-142");
    assert_eq!(w.pull_request_draft("BAU-142", "acme/nope"), None);
}

#[test]
fn a_run_with_several_repos_survives_json() {
    let w = done();
    let json = serde_json::to_string(&w).unwrap();
    assert_eq!(serde_json::from_str::<Workspace>(&json).unwrap(), w);
}
