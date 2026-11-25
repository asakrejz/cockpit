use super::*;

// ---------- Equal numbers from two repos ----------

/// Pull request `number` of `repo` as item, review and link.
fn in_repo(repo: &str, number: u32, title: &str) -> (Item, Review, PrLink) {
    let mut item = pr_item(number);
    item.id = format!("gh-{}-{number}", repo.replace('/', "-"));
    item.repo = Some(repo.into());
    item.pr.as_mut().unwrap().repo = repo.into();
    let mut r = review(number, files(&[(1, LineKind::Add, "x")]));
    r.repo = repo.into();
    r.title = title.into();
    let link = PrLink {
        number,
        repo: repo.into(),
        head_sha: "abc".into(),
    };
    (item, r, link)
}

fn two_repos(title_b: &str) -> Remote {
    let (ia, ra, la) = in_repo("acme/app", 5, "In app");
    let (ib, rb, lb) = in_repo("acme/web", 5, title_b);
    Remote {
        sources: vec![SourceData {
            source: Source::GitHub,
            outcome: Outcome::Loaded(Loaded {
                items: vec![ia, ib],
                reviews: vec![ra, rb],
                prs: vec![la, lb],
                projects: Vec::new(),
                warnings: Vec::new(),
            }),
        }],
    }
}

#[test]
fn equal_numbers_from_two_repos_stay_two_reviews() {
    let ws = synced(&two_repos("In web"));
    assert_eq!(ws.reviews.len(), 2);
    assert_eq!(
        ws.review(PrKey::new("acme/app", 5)).unwrap().title,
        "In app"
    );
    assert_eq!(
        ws.review(PrKey::new("acme/web", 5)).unwrap().title,
        "In web"
    );
    assert_eq!(
        ws.sync.pr(PrKey::new("acme/web", 5)).unwrap().repo,
        "acme/web"
    );
}

#[test]
fn local_state_stays_with_its_own_repo_across_syncs() {
    let mut ws = synced(&two_repos("In web"));
    let web = PrKey::new("acme/web", 5);
    ws.set_verdict(&web, Verdict::Approve);
    let (merged, _) = merge(&ws, &two_repos("In web, renamed"), T + 1);
    assert_eq!(merged.reviews.len(), 2);
    assert_eq!(merged.review(&web).unwrap().verdict, Verdict::Approve);
    assert_eq!(merged.review(&web).unwrap().title, "In web, renamed");
    assert_eq!(
        merged.review(PrKey::new("acme/app", 5)).unwrap().verdict,
        Verdict::default()
    );
}

#[test]
fn checks_changes_are_reported_for_the_right_repo() {
    let ws = synced(&two_repos("In web"));
    let mut next = two_repos("In web");
    if let Outcome::Loaded(l) = &mut next.sources[0].outcome {
        l.items[1].pr.as_mut().unwrap().checks = Checks::Failed;
    }
    let (_, changes) = merge(&ws, &next, T + 1);
    assert!(changes.iter().any(|c| matches!(
        c,
        Change::ChecksChanged { number, .. } if *number == PrKey::new("acme/web", 5)
    )));
    assert!(changes.iter().all(|c| match c {
        Change::ChecksChanged { number, .. } => number.repo == "acme/web",
        _ => true,
    }));
}

#[test]
fn a_state_without_repo_fields_still_works() {
    // Reviews and references without `repo`; the link `prs` carries the repo.
    let old = r##"{
        "items": [{"id":"gh-a-x-5","kind":"PullRequest","source":"GitHub","key":"#5","reason":"ReviewRequested",
                   "scope":"Mine","owner":{"name":"Lena","initials":"LK"},"repo":"acme/app",
                   "pr":{"number":5,"checks":"Passed"}}],
        "reviews": [{"number":5,"title":"Old","author":{"name":"Lena","initials":"LK"},"checks":"Passed"}],
        "sync": {"prs":[{"number":5,"repo":"acme/app","head_sha":"abc"}],"items":["gh-a-x-5"]}
    }"##;
    let ws: Workspace = serde_json::from_str(old).unwrap();
    assert_eq!(ws.reviews[0].repo, "");
    // A plain number finds the only review, and so does a key with the repo of its link.
    assert!(ws.review(5).is_some());
    assert!(ws.review(PrKey::new("acme/app", 5)).is_some());
    // An unknown repo is not written.
    assert!(
        !serde_json::to_string(&ws.reviews[0])
            .unwrap()
            .contains("\"repo\"")
    );
}
