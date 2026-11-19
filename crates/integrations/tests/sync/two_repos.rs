use super::*;

// ---------- Two repos with the same pull request number ----------

const REPO_B: &str = "acme/web";

/// Both repos have a pull request #87, with different titles and different heads.
async fn two_repo_server() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    search_none(&s).await;
    for (repo, title, sha) in [
        (REPO, "App: Avatar", "aaa111"),
        (REPO_B, "Web: Header", "ccc333"),
    ] {
        let base = format!("/repos/{repo}");
        get(
            &s,
            &format!("{base}/pulls"),
            ok(&json!([{
                "number": 87, "state": "open", "title": title, "body": "",
                "draft": false, "user": {"login": "tobias-lang"},
                "requested_reviewers": [{"login": "mkeller"}],
                "head": {"ref": "feature/x", "sha": sha},
                "base": {"ref": "main", "sha": "bbb222"},
                "created_at": "2024-05-13T08:00:00Z", "updated_at": "2024-05-14T10:00:00Z",
                "labels": []
            }])
            .to_string()),
        )
        .await;
        get(&s, &format!("{base}/pulls/87/files"), ok("[]")).await;
        get(&s, &format!("{base}/issues/87/comments"), ok("[]")).await;
        get(&s, &format!("{base}/pulls/87/comments"), ok("[]")).await;
        get(&s, &format!("{base}/pulls/87/reviews"), ok("[]")).await;
        get(
            &s,
            &format!("{base}/commits/{sha}/check-runs"),
            ok(fixture!("github_check_runs_ok.json")),
        )
        .await;
    }
    s
}

fn two_repo_services(g: &MockServer) -> Services {
    Services {
        github: Some(GitHubService {
            client: GitHubClient::new(&g.uri(), Secret::new("ghp_x")).unwrap(),
            repos: vec![REPO.into(), REPO_B.into()],
        }),
        jira: None,
    }
}

#[tokio::test]
async fn equal_pull_request_numbers_from_two_repos_both_arrive() {
    let g = two_repo_server().await;
    let d = two_repo_services(&g);
    let (ws, remote) = sync(&d, &Workspace::default(), NOW_MS).await;

    let (a, b) = (
        cockpit_core::PrKey::new(REPO, 87),
        cockpit_core::PrKey::new(REPO_B, 87),
    );
    assert_eq!(ws.reviews.len(), 2, "no pull request is skipped");
    assert_eq!(ws.review(&a).unwrap().title, "App: Avatar");
    assert_eq!(ws.review(&b).unwrap().title, "Web: Header");
    assert_eq!(ws.review(&a).unwrap().repo, REPO);
    assert_eq!(ws.sync.pr(&a).unwrap().head_sha, "aaa111");
    assert_eq!(ws.sync.pr(&b).unwrap().head_sha, "ccc333");
    // Entries have different IDs and each knows its repo.
    let items: Vec<_> = ws.items.iter().filter_map(|i| i.pr.as_ref()).collect();
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|p| p.key() == a) && items.iter().any(|p| p.key() == b));
    assert!(
        ws.sync
            .state_of(Source::GitHub)
            .unwrap()
            .warnings
            .is_empty()
    );
    let Outcome::Loaded(loaded) = &remote.sources[0].outcome else {
        panic!("loaded")
    };
    assert_eq!(loaded.prs.len(), 2);

    // Local state of one repo's #87 survives the next sync and never touches the other one.
    let mut ws = ws;
    ws.apply(
        Action::SetVerdict {
            review: b.clone(),
            verdict: Verdict::Approve,
        },
        Actor::User,
        NOW_MS + 1,
    )
    .unwrap();
    let (ws, _) = sync(&d, &ws, NOW_MS + 60_000).await;
    assert_eq!(ws.review(&b).unwrap().verdict, Verdict::Approve);
    assert_ne!(ws.review(&a).unwrap().verdict, Verdict::Approve);
}
