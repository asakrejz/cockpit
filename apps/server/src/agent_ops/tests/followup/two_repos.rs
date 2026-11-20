use super::*;

// ---------- Two repos with the same pull request number ----------

/// `pr_welt` plus a second repo `firma/web` that also has a pull request #85 (same branch name,
/// own comments) and is configured for the agent. The default repo stays `firma/app`.
fn two_repo_welt(github: &MockServer) -> World {
    let w = pr_welt(github);
    let source = w.tmp.path().join("source");
    let web = w.tmp.path().join("git/firma/web.git");
    sh(
        &source,
        &["push", "--quiet", web.to_str().unwrap(), PR_BRANCH],
    );
    let mut ws = w.ws();
    let mut review = ws.reviews.iter().find(|r| r.number == 85).cloned().unwrap();
    review.repo = "firma/web".into();
    review.title = "Web-Variante".into();
    ws.reviews.push(review);
    ws.sync.prs.push(PrLink {
        number: 85,
        repo: "firma/web".into(),
        head_sha: "def456".into(),
    });
    w.state.inner.live.set_state_for_test(ws);
    *w.state.inner.agent_repo_config.write().unwrap() = crate::app::RepoConfig {
        repos: vec![
            cockpit_core::AgentRepo::new("firma/app"),
            cockpit_core::AgentRepo::new("firma/web"),
        ],
        default: Some("firma/app".into()),
    };
    w
}

async fn comments_of(s: &MockServer, repo: &str, text: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/repos/{repo}/pulls/85/comments")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "user": {"login": "mira"}, "body": text, "path": "liste.txt", "line": 1}
        ])))
        .mount(s)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/{repo}/pulls/85/reviews")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(s)
        .await;
}

#[tokio::test]
async fn a_followup_reads_and_pushes_in_the_repo_of_its_pull_request() {
    let github = MockServer::start().await;
    comments_of(&github, "firma/app", "Nur in app").await;
    comments_of(&github, "firma/web", "Nur in web").await;
    let w = two_repo_welt(&github);
    w.write("scenario-run.sh", &scenario_followup());
    w.action(Action::AddressReviewThreads {
        review: cockpit_core::PrKey::new("firma/web", 85),
        options: direct(),
    })
    .await
    .unwrap();
    let key = "threads-firma~web-85";
    w.wait("fertig", |ws| {
        ws.runs.iter().any(|r| r.key() == key && r.is_done())
    })
    .await;
    w.wait_for_end().await;
    // The run belongs to web, the job carries web's comment only, and the push went to web.
    let run = w.ws().runs.into_iter().find(|r| r.key() == key).unwrap();
    assert_eq!(run.repo, "firma/web");
    let job = w.log("job-copy.json");
    assert!(
        job.contains("Nur in web") && !job.contains("Nur in app"),
        "{job}"
    );
    let log_of = |repo: &str| {
        let out = std::process::Command::new("git")
            .current_dir(w.tmp.path().join(format!("git/{repo}.git")))
            .args(["log", "--format=%s", PR_BRANCH])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert!(log_of("firma/web").starts_with("Folge"));
    assert!(
        !log_of("firma/app").contains("Folge"),
        "nothing pushed to the other repo"
    );
    // Only web's endpoints were read.
    let paths: Vec<String> = github
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect();
    assert!(paths.iter().all(|p| !p.contains("firma/app")), "{paths:?}");
    // The other repo's #85 can have its own run at the same time.
    assert!(
        w.ws()
            .runs
            .iter()
            .all(|r| r.key() != "threads-firma~app-85")
    );
}

#[tokio::test]
async fn the_first_use_of_a_repo_stores_its_settings_from_github() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/web"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "default_branch": "develop", "archived": false, "permissions": {"push": true}
        })))
        .mount(&github)
        .await;
    let w = two_repo_welt(&github);
    *w.state.inner.agent_repo_config.write().unwrap() = crate::app::RepoConfig {
        repos: vec![cockpit_core::AgentRepo::new("firma/app")],
        default: Some("firma/app".into()),
    };
    let action = Action::AddressReviewThreads {
        review: cockpit_core::PrKey::new("firma/web", 85),
        options: direct(),
    };
    w.state.repo_settings_prepare(&action).await.unwrap();
    let stored = w.state.agent_repos_config();
    let web = stored.find("firma/web").expect("stored");
    assert_eq!(web.base_branch, "develop");
    assert!(!web.watch, "the first use does not widen the sync");
    assert!(stored.find("firma/app").is_some());
    // The follow-up now branches off the repo's own default branch.
    assert_eq!(w.state.agent_repos().base_branch("firma/web"), "develop");
    // A second call changes nothing (and asks GitHub nothing).
    let asked = github.received_requests().await.unwrap().len();
    w.state.repo_settings_prepare(&action).await.unwrap();
    assert_eq!(github.received_requests().await.unwrap().len(), asked);
}

#[tokio::test]
async fn the_pull_request_goes_into_the_repo_the_run_worked_in() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/web/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&github)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/firma/web/pulls"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "number": 7, "html_url": "https://github.example/firma/web/pull/7", "state": "open"
        })))
        .expect(1)
        .mount(&github)
        .await;
    let w = two_repo_welt(&github);
    w.state.inner.live.set_state_for_test(demo::workspace());
    w.write("scenario-run.sh", &scenario_ok());
    w.action(Action::StartAgent {
        ticket: "BAU-142".into(),
        options: AgentOptions {
            plan_first: false,
            repo: Some("firma/web".into()),
            ..AgentOptions::default()
        },
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.run("BAU-142").is_some_and(|r| r.is_done())
    })
    .await;
    w.wait_for_end().await;
    let p = w.state.pull_request_preview("BAU-142", "").await.unwrap();
    assert_eq!(p.repo, "firma/web");
    w.action(create("BAU-142: Web")).await.unwrap();
    let pull = w.run().pull_request.unwrap();
    assert_eq!(pull.url, "https://github.example/firma/web/pull/7");
    // Nothing was asked of or sent to the other repo.
    let hits_app = github
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().contains("firma/app"))
        .count();
    assert_eq!(hits_app, 0);
}
