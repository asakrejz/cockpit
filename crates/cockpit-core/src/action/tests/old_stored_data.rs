//! Old saved runs, comments and actions that must keep loading.

use super::*;

#[test]
fn foreign_comments_stay_out_of_my_review_and_cannot_be_edited() {
    let mut w = ws();
    let r = w.review(91).unwrap();
    assert_eq!(r.comment_count(), 0, "comments of others do not go out");
    fails(
        &mut w,
        Action::EditLineComment {
            review: 91.into(),
            comment: "c1".into(),
            text: "Anders".into(),
            suggestion: None,
        },
        ActionError::InvalidInput,
    );
    fails(
        &mut w,
        Action::DeleteLineComment {
            review: 91.into(),
            comment: "c1".into(),
        },
        ActionError::UnknownComment("c1".into()),
    );
    // Own comments that are marked as done (by a run) do not go out again.
    let mut r = w.review(87).unwrap().clone();
    r.add_line_comment("tokens.css", 1, "Eigener", None)
        .unwrap();
    assert_eq!(r.comment_count(), 1);
    r.line_comments.last_mut().unwrap().resolved = Some("review-87".into());
    assert_eq!(r.comment_count(), 0);
}

#[test]
fn the_follow_up_actions_survive_json() {
    for a in [
        Action::AddressReviewThreads {
            review: 91.into(),
            options: direct(),
        },
        Action::FixFailedChecks {
            review: 85.into(),
            options: direct(),
        },
    ] {
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
    }
}

#[test]
fn old_runs_and_comments_still_load() {
    // Old runs without `kind` are „implement ticket“.
    let old: crate::agent::AgentRun =
        serde_json::from_str(r#"{"ticket_key":"BAU-1","state":"Done"}"#).unwrap();
    assert_eq!(old.kind, crate::agent::RunKind::Implement);
    assert_eq!(old.key(), "BAU-1");
    // Old comments without an author belong to me and are open.
    let c: crate::review::LineComment =
        serde_json::from_str(r#"{"id":"c1","file":"a","line":0,"text":"x"}"#).unwrap();
    assert!(c.is_own() && c.is_open());
}

#[test]
fn a_followup_counts_for_the_team_and_nobody_but_you_merges() {
    let mut w = ws();
    ok(
        &mut w,
        Action::AddressReviewThreads {
            review: 91.into(),
            options: direct(),
        },
        1,
    );
    report(
        &mut w,
        "threads-acme~app-91",
        AgentReport::Started {
            detail: "docker".into(),
        },
    )
    .unwrap();
    let team = crate::team::members(&w);
    let agent = team.iter().find(|m| m.person.is_agent()).unwrap();
    assert_eq!(
        (agent.in_progress, agent.review),
        (1, 0),
        "BAU-138 counts once, now in progress"
    );
    assert_eq!(agent.item_ids, ["bau138"]);
    assert_eq!(
        w.apply(
            Action::MergePr {
                review: 91.into(),
                method: MergeMethod::Squash
            },
            Actor::Agent,
            T
        ),
        Err(ActionError::MergeNotAllowed(Actor::Agent))
    );
}
