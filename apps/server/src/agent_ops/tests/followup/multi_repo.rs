use super::*;
use cockpit_core::RunRepoState;

// ---------- One run in two repos ----------

/// The world with `firma/app` and `firma/web` both configured for the agent.
fn two_welt(github: &MockServer) -> World {
    let w = pr_welt(github);
    *w.state.inner.agent_repo_config.write().unwrap() = crate::app::RepoConfig {
        repos: vec![
            cockpit_core::AgentRepo::new("firma/app"),
            cockpit_core::AgentRepo::new("firma/web"),
        ],
        default: Some("firma/app".into()),
    };
    let mut ws = demo::workspace();
    // A path rule only `web` has.
    ws.set_repo_rules(
        "firma/web",
        cockpit_core::RepoRules {
            forbidden_paths: vec!["secrets/".into()],
            ..Default::default()
        },
    );
    w.state.inner.live.set_state_for_test(ws);
    w
}

/// `app` gets a new file; what `web` does comes from `web_part`.
fn scenario(web_part: &str) -> String {
    format!(
        r#"for a in "$@"; do case "$a" in *:/job:ro) job="${{a%%:/job:ro}}";; esac; done
cp "$job/job.json" "$d/job-copy.json"
echo neu > "$clone/firma__app/neu.txt"
git -C "$clone/firma__app" add -A
git -C "$clone/firma__app" -c user.name=a -c user.email=a@a.invalid commit -qm Agent
{web_part}
printf '%s\n' \
 '{{"t":"step","title":"Datei ändern","text":"neu.txt","tokens_in":100,"tokens_out":20,"ms":500}}' \
 '{{"t":"result","text":"In zwei Repos geändert.","branch":"{BRANCH}","commit":"abc1234","repos":[{{"repo":"firma/app","branch":"{BRANCH}","commit":"abc1234"}}]}}'
exit 0
"#
    )
}

fn start_two() -> Action {
    Action::StartAgent {
        ticket: "BAU-142".into(),
        options: AgentOptions {
            plan_first: false,
            repos: vec!["firma/app".into(), "firma/web".into()],
            ..AgentOptions::default()
        },
    }
}

async fn run_two(w: &World) {
    w.action(start_two()).await.unwrap();
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
}

#[tokio::test]
async fn a_changed_and_an_unchanged_repo_push_only_the_changed_one() {
    let github = MockServer::start().await;
    let w = two_welt(&github);
    w.write("scenario-run.sh", &scenario(""));
    run_two(&w).await;
    // Only `app` got a branch; `web` is left alone.
    assert_eq!(w.branches_of("app"), [PR_BRANCH, BRANCH, "main"]);
    assert_eq!(w.branches_of("web"), ["main"]);
    let run = w.run();
    assert_eq!(run.repos.len(), 2);
    assert_eq!(run.repos[0].state, RunRepoState::Pushed);
    assert_eq!(run.repos[0].files, ["neu.txt"]);
    assert_eq!(run.repos[1].state, RunRepoState::Unchanged);
    assert!(run.summary.contains("firma/app") && run.summary.contains("firma/web"));
    // Both clones are in the one mount, with the agent's instructions naming both.
    let calls = w.log("calls.log");
    assert!(
        calls
            .lines()
            .any(|z| z.starts_with("run ") && z.contains("/run:/work")),
        "{calls}"
    );
    let job: serde_json::Value = serde_json::from_str(&w.log("job-copy.json")).unwrap();
    let dirs: Vec<&str> = job["repos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["dir"].as_str().unwrap())
        .collect();
    assert_eq!(dirs, ["firma__app", "firma__web"]);
    assert_eq!(job["repos"][1]["rules"]["forbidden_paths"][0], "secrets/");
    assert_eq!(job["repos"][0]["branch"], BRANCH);
    // Nothing went to GitHub: pull requests wait for the click.
    assert!(github.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_repo_that_breaks_its_path_rules_is_refused_and_the_other_still_goes_out() {
    let github = MockServer::start().await;
    let w = two_welt(&github);
    w.write(
        "scenario-run.sh",
        &scenario(
            r#"mkdir -p "$clone/firma__web/secrets"
echo geheim > "$clone/firma__web/secrets/key.txt"
git -C "$clone/firma__web" add -A
git -C "$clone/firma__web" -c user.name=a -c user.email=a@a.invalid commit -qm Agent"#,
        ),
    );
    run_two(&w).await;
    assert_eq!(w.branches_of("app"), [PR_BRANCH, BRANCH, "main"]);
    assert_eq!(
        w.branches_of("web"),
        ["main"],
        "the refused repo got nothing pushed"
    );
    let run = w.run();
    assert_eq!(run.repos[0].state, RunRepoState::Pushed);
    assert_eq!(run.repos[1].state, RunRepoState::Refused);
    assert!(
        run.repos[1].reason.contains("secrets/key.txt"),
        "{}",
        run.repos[1].reason
    );
    assert!(
        run.summary.contains("Abgelehnt: firma/web"),
        "{}",
        run.summary
    );
}

#[tokio::test]
async fn pull_requests_are_created_per_repo_and_link_each_other() {
    let github = MockServer::start().await;
    for repo in ["firma/app", "firma/web"] {
        Mock::given(method("GET"))
            .and(path(format!("/repos/{repo}/pulls")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&github)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/repos/firma/app/pulls"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "number": 31, "html_url": "https://github.example/firma/app/pull/31", "state": "open"
        })))
        .expect(1)
        .mount(&github)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/firma/web/pulls"))
        .and(wiremock::matchers::body_string_contains("firma/app#31"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "number": 7, "html_url": "https://github.example/firma/web/pull/7", "state": "open"
        })))
        .expect(1)
        .mount(&github)
        .await;
    let w = two_welt(&github);
    w.write(
        "scenario-run.sh",
        &scenario(
            r#"echo web > "$clone/firma__web/web.txt"
git -C "$clone/firma__web" add -A
git -C "$clone/firma__web" -c user.name=a -c user.email=a@a.invalid commit -qm Agent"#,
        ),
    );
    run_two(&w).await;
    assert_eq!(w.branches_of("web"), [BRANCH, "main"]);
    let preview = w
        .state
        .pull_request_preview("BAU-142", "firma/web")
        .await
        .unwrap();
    assert_eq!(preview.repo, "firma/web");
    // Creating for a repo that is not part of the run fails.
    let unknown = w.state.pull_request_preview("BAU-142", "firma/other").await;
    assert!(unknown.is_err());
    let create_in = |repo: &str| Action::CreatePullRequest {
        run: "BAU-142".into(),
        title: "BAU-142: Toggle".into(),
        body: "Text".into(),
        pull: None,
        repo: repo.into(),
    };
    w.action(create_in("firma/app")).await.unwrap();
    // The text of the second one is built from the draft, so it carries the link to the first.
    let second = w
        .state
        .pull_request_preview("BAU-142", "firma/web")
        .await
        .unwrap();
    assert!(second.body.contains("firma/app#31"), "{}", second.body);
    let mut a = create_in("firma/web");
    if let Action::CreatePullRequest { body, .. } = &mut a {
        *body = second.body;
    }
    w.action(a).await.unwrap();
    let run = w.run();
    assert_eq!(run.repos[0].pr_number(), Some(31));
    assert_eq!(run.repos[1].pr_number(), Some(7));
    assert!(run.repos.iter().all(|r| r.state == RunRepoState::PrCreated));
    // A second click for the same repo does not create a duplicate.
    assert!(w.action(create_in("firma/web")).await.is_err());
}
