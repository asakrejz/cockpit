//! Equal pull request numbers from two repos stay two pull requests.

use super::*;
use crate::DEMO_REPO;
use crate::demo::DEMO_REPO_TOKENS;
use crate::model::PrKey;

/// The demo plus `#87` in the second repo.
fn two() -> Workspace {
    demo::multi_repo_workspace()
}

fn first() -> PrKey {
    PrKey::new(DEMO_REPO, 87)
}

fn second() -> PrKey {
    PrKey::new(DEMO_REPO_TOKENS, 87)
}

#[test]
fn the_seed_has_the_same_number_in_two_repos() {
    let w = two();
    assert_eq!(w.review(first()).unwrap().repo, DEMO_REPO);
    assert_eq!(w.review(second()).unwrap().repo, DEMO_REPO_TOKENS);
    assert_ne!(
        w.review(first()).unwrap().title,
        w.review(second()).unwrap().title
    );
}

#[test]
fn state_and_actions_stay_in_their_repo() {
    let mut w = two();
    // Decide a suggestion of the first repo's #87 and set the verdict of the second one's.
    ok(
        &mut w,
        Action::Decide {
            review: first(),
            suggestion: "a".into(),
            decision: Some(Decision::Approved),
        },
        1,
    );
    ok(
        &mut w,
        Action::SetVerdict {
            review: second(),
            verdict: Verdict::Approve,
        },
        1,
    );
    assert_eq!(w.review(first()).unwrap().approved(), 1);
    assert_eq!(w.review(first()).unwrap().verdict, Verdict::Comment);
    assert_eq!(w.review(second()).unwrap().verdict, Verdict::Approve);
    assert_eq!(w.review(second()).unwrap().approved(), 0);
}

#[test]
fn the_log_names_the_repo_and_points_at_the_right_review() {
    let mut w = two();
    ok(
        &mut w,
        Action::SetVerdict {
            review: second(),
            verdict: Verdict::Approve,
        },
        1,
    );
    let entry = w.log.last().unwrap();
    assert_eq!(entry.target, Some(Target::Review(second())));
    assert!(
        entry.text.contains("acme/design-tokens#87"),
        "{}",
        entry.text
    );
}

#[test]
fn an_unknown_repo_is_not_the_other_one() {
    let mut w = two();
    let other = PrKey::new("acme/else", 87);
    fails(
        &mut w,
        Action::SetVerdict {
            review: other.clone(),
            verdict: Verdict::Approve,
        },
        ActionError::UnknownReview(other),
    );
}

#[test]
fn merging_one_leaves_the_other_open() {
    let mut w = two();
    let second_item = w.item("pr87-tokens").unwrap().pr.clone().unwrap();
    assert_eq!(second_item.key(), second());
    // Only the first #87 is ready to merge (reviewed and sent).
    for s in ["a", "b", "c"] {
        ok(
            &mut w,
            Action::Decide {
                review: first(),
                suggestion: s.into(),
                decision: Some(Decision::Declined),
            },
            1,
        );
    }
    ok(
        &mut w,
        Action::SendReview {
            review: first(),
            comment: String::new(),
        },
        1,
    );
    ok(
        &mut w,
        Action::MergePr {
            review: first(),
            method: MergeMethod::Squash,
        },
        1,
    );
    assert!(w.review(first()).unwrap().merged.is_some());
    assert!(w.review(second()).unwrap().merged.is_none());
    assert!(!w.item("pr87-tokens").unwrap().pr.as_ref().unwrap().merged);
    assert!(w.item("pr87").unwrap().pr.as_ref().unwrap().merged);
}

#[test]
fn follow_up_runs_are_named_with_the_repo() {
    let mut w = two();
    // A red check in the second repo only.
    let key = w.review(second()).unwrap().key();
    let run = AgentRun::followup(
        RunKind::AddressReview {
            review: key,
            comments: vec![],
        },
        "x",
        "t",
        "b",
    );
    assert_eq!(run.key(), "review-acme~design-tokens-87");
    let other = AgentRun::followup(
        RunKind::AddressReview {
            review: first(),
            comments: vec![],
        },
        "x",
        "t",
        "b",
    );
    assert_ne!(run.key(), other.key());
    assert_eq!(run.pr_key(), Some(second()));
    w.runs.push(run);
    assert_eq!(w.followups(second()).len(), 1);
    assert!(w.followups(first()).is_empty());
}

#[test]
fn a_started_run_works_in_the_chosen_repo() {
    let mut w = two();
    let options = AgentOptions {
        repo: Some(DEMO_REPO_TOKENS.into()),
        ..direct()
    };
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options,
        },
        1,
    );
    assert_eq!(w.run("BAU-142").unwrap().repo, DEMO_REPO_TOKENS);
    // Finishing creates the pull request in that repo, not in the other.
    for _ in 0..STEPS * 2 {
        let _ = w.apply(Action::AdvanceAgents, Actor::System, T);
        let _ = w.apply(
            Action::AnswerAgent {
                run: "BAU-142".into(),
                text: "Entfernen".into(),
            },
            Actor::User,
            T,
        );
    }
    let run = w.run("BAU-142").unwrap();
    assert!(run.is_done());
    let key = run.pr_key().unwrap();
    assert_eq!(key.repo, DEMO_REPO_TOKENS);
    assert_eq!(w.review(&key).unwrap().repo, DEMO_REPO_TOKENS);
    assert_eq!(
        w.item("bau142").unwrap().pr.as_ref().unwrap().repo,
        DEMO_REPO_TOKENS
    );
}

#[test]
fn old_actions_with_plain_numbers_still_apply() {
    let mut w = ws();
    let old: Action =
        serde_json::from_str(r#"{"SetVerdict":{"review":87,"verdict":"Approve"}}"#).unwrap();
    ok(&mut w, old, 1);
    assert_eq!(w.review(87).unwrap().verdict, Verdict::Approve);
    let new: Action = serde_json::from_str(
        &serde_json::to_string(&Action::SetVerdict {
            review: first(),
            verdict: Verdict::Approve,
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        new,
        Action::SetVerdict {
            review: first(),
            verdict: Verdict::Approve
        }
    );
}

#[test]
fn old_log_entries_with_a_number_still_load() {
    let entry: LogEntry = serde_json::from_str(
        r#"{"id":1,"at_ms":5,"actor":"User","target":{"Review":87},"text":"x"}"#,
    )
    .unwrap();
    assert_eq!(entry.target, Some(Target::Review(87.into())));
}

#[test]
fn real_followups_on_equal_numbers_are_separate_runs_in_their_repos() {
    let mut w = two();
    for r in &mut w.reviews {
        if r.number == 87 {
            r.branch = format!("claude/x-{}", r.repo.replace('/', "-"));
        }
    }
    for key in [first(), second()] {
        ok(
            &mut w,
            Action::AddressReviewThreads {
                review: key,
                options: direct(),
            },
            1,
        );
    }
    let keys: Vec<String> = w.runs.iter().map(AgentRun::key).collect();
    assert!(
        keys.contains(&"threads-acme~app-87".to_string()),
        "{keys:?}"
    );
    assert!(
        keys.contains(&"threads-acme~design-tokens-87".to_string()),
        "{keys:?}"
    );
    // Each run works in its own repo and on its own branch.
    for run in w.runs.iter().filter(|r| r.is_followup()) {
        assert_eq!(run.pr_key().unwrap().repo, run.repo);
        assert!(run.target_branch.ends_with(&run.repo.replace('/', "-")));
    }
    // The log entries point at the right review.
    let targets: Vec<_> = w.log.iter().filter_map(|e| e.target.clone()).collect();
    assert!(
        targets.contains(&Target::Review(first())) && targets.contains(&Target::Review(second()))
    );
    // A second job on the same pull request is refused, on the other repo's one it is not.
    fails(
        &mut w,
        Action::AddressReviewThreads {
            review: second(),
            options: direct(),
        },
        ActionError::Agent(crate::workspace::AgentError::AlreadyRunning),
    );
}
