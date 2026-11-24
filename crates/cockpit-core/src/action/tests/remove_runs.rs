use super::*;
use crate::agent::Closure;
use crate::usage::{Charge, Usage};

fn run(key: &str, state: RunState) -> AgentRun {
    let mut run = AgentRun::new(key, "Sample");
    run.state = state;
    run
}

fn workspace(states: &[(&str, RunState)]) -> Workspace {
    Workspace {
        runs: states.iter().map(|(k, s)| run(k, *s)).collect(),
        ..Workspace::default()
    }
}

fn remove(w: &mut Workspace, key: &str) -> Result<Applied, ActionError> {
    w.apply(Action::RemoveAgentRun { run: key.into() }, Actor::User, T)
}

#[test]
fn final_runs_can_be_removed_one_by_one() {
    let mut w = workspace(&[
        ("A-1", RunState::Done),
        ("A-2", RunState::Idle),
        ("A-3", RunState::Stopped(StopReason::Time)),
    ]);
    for key in ["A-1", "A-2", "A-3"] {
        let applied = remove(&mut w, key).unwrap();
        assert_eq!(applied.logged, 1);
        assert!(w.runs.iter().all(|r| r.key() != key));
    }
    assert!(w.runs.is_empty());
    assert_eq!(
        w.log.last().unwrap().target,
        Some(Target::Run("A-3".into()))
    );
}

#[test]
fn a_finished_run_with_a_merged_pull_request_can_be_removed() {
    let mut r = run("A-1", RunState::Done);
    r.closure = Some(Closure::Merged);
    let mut w = Workspace {
        runs: vec![r],
        ..Workspace::default()
    };
    remove(&mut w, "A-1").unwrap();
    assert!(w.runs.is_empty());
}

#[test]
fn working_and_waiting_runs_cannot_be_removed() {
    for state in [
        RunState::Running,
        RunState::AwaitingApproval,
        RunState::AwaitingAnswer,
        RunState::Paused,
    ] {
        let mut w = workspace(&[("A-1", state)]);
        fails(
            &mut w,
            Action::RemoveAgentRun { run: "A-1".into() },
            ActionError::RunStillActive("A-1".into()),
        );
        assert_eq!(w.runs.len(), 1, "{state:?}");
    }
}

#[test]
fn an_unknown_run_is_nothing_to_do() {
    let mut w = workspace(&[]);
    fails(
        &mut w,
        Action::RemoveAgentRun { run: "A-9".into() },
        ActionError::NothingToDo,
    );
}

#[test]
fn only_the_user_may_remove_runs() {
    let mut w = workspace(&[("A-1", RunState::Done)]);
    for action in [
        Action::RemoveAgentRun { run: "A-1".into() },
        Action::RemoveFinishedRuns,
    ] {
        for actor in [Actor::System, Actor::Agent] {
            assert_eq!(
                w.apply(action.clone(), actor, T),
                Err(ActionError::UserOnly(actor))
            );
        }
    }
    assert_eq!(w.runs.len(), 1);
}

#[test]
fn remove_finished_keeps_everything_that_still_works() {
    let mut w = workspace(&[
        ("A-1", RunState::Done),
        ("A-2", RunState::Running),
        ("A-3", RunState::Idle),
        ("A-4", RunState::Paused),
        ("A-5", RunState::Stopped(StopReason::MonthBudget)),
        ("A-6", RunState::AwaitingApproval),
    ]);
    let applied = w.apply(Action::RemoveFinishedRuns, Actor::User, T).unwrap();
    assert_eq!(applied.logged, 1);
    assert!(applied.summary.contains('3'), "{}", applied.summary);
    let left: Vec<String> = w.runs.iter().map(|r| r.key()).collect();
    assert_eq!(left, ["A-2", "A-4", "A-6"]);
}

#[test]
fn remove_finished_without_finished_runs_changes_nothing() {
    let mut w = workspace(&[("A-1", RunState::Running)]);
    fails(&mut w, Action::RemoveFinishedRuns, ActionError::NothingToDo);
}

#[test]
fn removing_runs_keeps_the_spend_and_the_budget_stays_correct() {
    let mut w = workspace(&[("A-1", RunState::Done), ("A-2", RunState::Done)]);
    w.spend.push(Charge {
        at_ms: T,
        ticket: "A-1".into(),
        usage: Usage {
            cost_cents: 250,
            ..Usage::default()
        },
    });
    let month = w.spent_month(T);
    assert_eq!(month.cost_cents, 250);
    remove(&mut w, "A-1").unwrap();
    w.apply(Action::RemoveFinishedRuns, Actor::User, T).unwrap();
    assert!(w.runs.is_empty());
    assert_eq!(w.spent_month(T), month, "the cost of removed runs stays");
}

#[test]
fn removed_runs_stay_gone_after_a_sync_and_a_reload() {
    let mut w = workspace(&[("A-1", RunState::Done), ("A-2", RunState::Running)]);
    remove(&mut w, "A-1").unwrap();
    let _ = w.apply(
        Action::SyncRemote {
            remote: crate::sync::Remote {
                sources: Vec::new(),
            },
        },
        Actor::System,
        T + 1,
    );
    let json = persist::save(&w);
    let back = persist::load(&json).unwrap();
    let left: Vec<String> = back.runs.iter().map(|r| r.key()).collect();
    assert_eq!(left, ["A-2"]);
}
