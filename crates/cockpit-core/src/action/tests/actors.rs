//! Who may apply which action: the rule lives in `Action::required_actor` and is enforced by
//! `Workspace::apply`, before anything else.

use super::*;
use crate::agent::{AgentReport, Closure};
use crate::model::Person;

/// Everything that reaches Jira or GitHub, with an item and a pull request of the demo data.
fn outward() -> Vec<Action> {
    vec![
        Action::Reply {
            item: "bau142".into(),
            text: "Hello".into(),
        },
        Action::SetStatus {
            item: "bau142".into(),
            status: TicketStatus::Review,
        },
        Action::CreateBranch {
            item: "bau142".into(),
        },
        Action::EditTicket {
            item: "bau142".into(),
            title: "New title".into(),
            description: String::new(),
        },
        Action::AssignTicket {
            item: "bau142".into(),
            person: Person::me(),
        },
        Action::SetPriority {
            item: "bau142".into(),
            priority: Priority::High,
        },
        Action::SendReview {
            review: 87.into(),
            comment: "Looks good".into(),
        },
        Action::RestartCheck {
            review: 87.into(),
            check: "build".into(),
        },
    ]
}

#[test]
fn only_the_user_may_trigger_what_reaches_jira_or_github() {
    for action in outward() {
        assert_eq!(
            action.required_actor(),
            Some(Actor::User),
            "{action:?} must be user-only"
        );
        let mut w = ws();
        let before = w.clone();
        for actor in [Actor::Agent, Actor::System] {
            assert_eq!(
                w.apply(action.clone(), actor, T),
                Err(ActionError::UserOnly(actor)),
                "{action:?} as {actor:?}"
            );
        }
        assert_eq!(
            w, before,
            "a refused action changes nothing and logs nothing"
        );
    }
}

#[test]
fn the_user_is_not_turned_away_by_the_actor_rule() {
    for action in outward() {
        let mut w = ws();
        let result = w.apply(action.clone(), Actor::User, T);
        assert!(
            !matches!(
                result,
                Err(ActionError::UserOnly(_) | ActionError::MergeNotAllowed(_))
            ),
            "{action:?}: {result:?}"
        );
    }
}

#[test]
fn merging_keeps_its_own_message() {
    let action = Action::MergePr {
        review: 87.into(),
        method: MergeMethod::Merge,
    };
    assert_eq!(action.required_actor(), Some(Actor::User));
    assert_eq!(
        ws().apply(action, Actor::System, T),
        Err(ActionError::MergeNotAllowed(Actor::System))
    );
}

#[test]
fn the_backend_and_the_agent_keep_their_own_actions() {
    let sync = Action::SyncRemote {
        remote: Remote::default(),
    };
    let close = Action::CloseAgentRun {
        run: "BAU-142".into(),
        closure: Closure::Merged,
    };
    let report = Action::ReportAgent {
        run: "BAU-142".into(),
        report: AgentReport::Started {
            detail: String::new(),
        },
    };
    assert_eq!(sync.required_actor(), Some(Actor::System));
    assert_eq!(close.required_actor(), Some(Actor::System));
    assert_eq!(report.required_actor(), Some(Actor::Agent));
    for actor in [Actor::User, Actor::Agent] {
        assert_eq!(
            ws().apply(sync.clone(), actor, T),
            Err(ActionError::SystemOnly)
        );
        assert_eq!(
            ws().apply(close.clone(), actor, T),
            Err(ActionError::SystemOnly)
        );
    }
    for actor in [Actor::User, Actor::System] {
        assert_eq!(
            ws().apply(report.clone(), actor, T),
            Err(ActionError::AgentOnly)
        );
    }
}

#[test]
fn the_ticks_run_as_the_system() {
    for action in [
        Action::WakeSnoozed,
        Action::AutoLinkPrs,
        Action::SyncTicketStatus,
    ] {
        assert_eq!(action.required_actor(), None, "{action:?}");
    }
    let mut w = ws();
    for (action, actor) in [
        (Action::WakeSnoozed, Actor::System),
        (Action::AutoLinkPrs, Actor::System),
        (Action::SyncTicketStatus, Actor::System),
    ] {
        let result = w.apply(action.clone(), actor, T);
        assert!(
            !matches!(
                result,
                Err(ActionError::UserOnly(_)
                    | ActionError::SystemOnly
                    | ActionError::AgentOnly
                    | ActionError::MergeNotAllowed(_))
            ),
            "{action:?}: {result:?}"
        );
    }
}
