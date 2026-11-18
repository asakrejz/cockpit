use super::*;

#[tokio::test]
async fn review_comments_run_on_the_same_branch_and_push_normally() {
    let github = MockServer::start().await;
    comments_ok(&github).await;
    let w = pr_welt(&github);
    w.write("scenario-run.sh", &scenario_followup());
    w.action(Action::AddressReviewThreads {
        review: 85.into(),
        options: direct(),
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.runs
            .iter()
            .any(|r| r.key() == "threads-firma~app-85" && r.is_done())
    })
    .await;
    w.wait_for_end().await;

    // New commit on the branch of the pull request, history kept (normal push).
    let log = remote_log(&w).await;
    assert!(
        log.starts_with("Folge") && log.contains("Agent früher"),
        "{log}"
    );
    assert_eq!(w.branches(), [PR_BRANCH, "main"]);

    // The job file carries the threads, the review text and the branch – nothing secret.
    let job: serde_json::Value =
        serde_json::from_str(&w.log("job-copy.json")).expect("job.json was copied");
    assert_eq!(job["branch"], PR_BRANCH);
    assert_eq!(job["followup"]["kind"], "review_comments");
    assert_eq!(job["followup"]["pull_request"], 85);
    assert_eq!(
        job["followup"]["threads"][0]["comments"][1]["text"],
        "Mach ich"
    );
    assert_eq!(job["followup"]["notes"][0]["text"], "Noch Tests");
    let text = w.log("job-copy.json");
    assert!(!text.contains(GITHUB) && !text.contains(TOKEN), "{text}");

    // Security: the container process got the model key, but never the GitHub token.
    let env = w.log("env-all.log");
    assert!(!env.contains(GITHUB), "GitHub-Token im Container: {env}");
    let calls = w.log("calls.log");
    assert!(!calls.contains(GITHUB) && !calls.contains(TOKEN), "{calls}");
    assert!(!env.contains("GITHUB_TOKEN") && !env.contains("GH_TOKEN"));

    // The report looks like that of a normal run.
    let run = w
        .ws()
        .runs
        .into_iter()
        .find(|r| r.key() == "threads-firma~app-85")
        .unwrap();
    assert!(run.summary.contains("Einzug angepasst."));
    assert!(run.usage().input_tokens >= 100);
    assert_eq!(run.branch(), PR_BRANCH);
    assert!(w.ws().log.iter().all(|e| !e.text.contains(GITHUB)));
}

#[tokio::test]
async fn failed_checks_go_to_the_agent_with_a_shortened_excerpt() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "number": 85, "title": "t", "user": {"login": "ich"},
            "head": {"ref": PR_BRANCH, "sha": "abc123"}, "base": {"ref": "main"}
        })))
        .mount(&github)
        .await;
    let long = format!("{}FEHLER-AM-ENDE", "x".repeat(20_000));
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/commits/abc123/check-runs"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"check_runs": [
                {"id": 7, "name": "Lint", "status": "completed", "conclusion": "failure",
                 "output": {"title": "rot", "summary": long}},
                {"id": 8, "name": "Build", "status": "completed", "conclusion": "success"}
            ]})),
        )
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/check-runs/7/annotations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&github)
        .await;
    let w = pr_welt(&github);
    w.write("scenario-run.sh", &scenario_followup());
    w.action(Action::FixFailedChecks {
        review: 85.into(),
        options: direct(),
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.runs
            .iter()
            .any(|r| r.key() == "fixes-firma~app-85" && r.is_done())
    })
    .await;
    w.wait_for_end().await;
    let job: serde_json::Value = serde_json::from_str(&w.log("job-copy.json")).unwrap();
    assert_eq!(job["followup"]["kind"], "failed_checks");
    let checks = job["followup"]["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0]["name"], "Lint");
    let excerpt = checks[0]["excerpt"].as_str().unwrap();
    assert!(excerpt.chars().count() <= 3_000, "{}", excerpt.len());
    assert!(excerpt.ends_with("FEHLER-AM-ENDE"));
    assert!(remote_log(&w).await.starts_with("Folge"));
}

#[tokio::test]
async fn secrets_in_review_texts_do_not_reach_the_job_file() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "user": {"login": "x"}, "body": format!("Mein Token ist {GITHUB}"), "path": "a", "line": 1}
        ])))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&github)
        .await;
    let w = pr_welt(&github);
    w.write("scenario-run.sh", &scenario_followup());
    w.action(Action::AddressReviewThreads {
        review: 85.into(),
        options: direct(),
    })
    .await
    .unwrap();
    w.wait("fertig", |ws| {
        ws.runs
            .iter()
            .any(|r| r.key() == "threads-firma~app-85" && r.is_done())
    })
    .await;
    w.wait_for_end().await;
    let text = w.log("job-copy.json");
    assert!(text.contains("Mein Token ist ***"), "{text}");
    assert!(!text.contains(GITHUB));
}

#[tokio::test]
async fn nothing_to_do_is_refused_before_a_container_starts() {
    let github = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/firma/app/pulls/85/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&github)
        .await;
    let w = pr_welt(&github);
    let e = w
        .action(Action::AddressReviewThreads {
            review: 85.into(),
            options: direct(),
        })
        .await
        .unwrap_err();
    assert!(e.contains("keine Review-Kommentare"), "{e}");
    assert!(
        w.ws()
            .runs
            .iter()
            .all(|r| r.key() != "threads-firma~app-85")
    );
    assert!(!w.log("calls.log").contains("run "));
}

#[tokio::test]
async fn github_errors_on_the_inputs_are_told_in_plain_words() {
    for (status, needle) in [(401, "401"), (403, "403")] {
        let github = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"message": "x"})))
            .mount(&github)
            .await;
        let w = pr_welt(&github);
        let e = w
            .action(Action::AddressReviewThreads {
                review: 85.into(),
                options: direct(),
            })
            .await
            .unwrap_err();
        assert!(e.contains(needle) && e.contains("Review-Kommentare"), "{e}");
        assert!(!e.contains(GITHUB));
    }
}

#[tokio::test]
async fn followups_need_the_real_agent_to_be_ready_and_an_agent_branch() {
    // Not set up: the message says what is missing.
    let github = MockServer::start().await;
    let w = pr_welt(&github);
    std::fs::remove_file(w.bin().join("docker")).unwrap();
    let e = w
        .action(Action::FixFailedChecks {
            review: 85.into(),
            options: direct(),
        })
        .await
        .unwrap_err();
    assert!(
        e.contains("noch nicht starten") && e.contains("Schritt „Container“"),
        "{e}"
    );
    // A person's branch is refused by the rules of the workspace.
    let w2 = pr_welt(&github);
    let mut ws = w2.ws();
    for r in ws.reviews.iter_mut().filter(|r| r.number == 85) {
        r.branch = "feature/x".into();
    }
    w2.state.inner.live.set_state_for_test(ws);
    comments_ok(&github).await;
    let e = w2
        .action(Action::AddressReviewThreads {
            review: 85.into(),
            options: direct(),
        })
        .await
        .unwrap_err();
    assert!(e.contains("claude/"), "{e}");
}
