use super::*;
use crate::agent::Closure;
use cockpit_i18n::Lang;

fn finished_run() -> AgentRun {
    let mut run = AgentRun::new("BAU-138", "Sample");
    run.state = RunState::Done;
    run.repo = "acme/app".into();
    run.pull_request = Some(PullRequestRef {
        number: 7,
        url: String::new(),
        existed: false,
    });
    run
}

fn with_run(run: AgentRun) -> Workspace {
    Workspace {
        runs: vec![run],
        ..Workspace::default()
    }
}

fn close(w: &mut Workspace, run: &str, closure: Closure) -> Result<Applied, ActionError> {
    w.apply(
        Action::CloseAgentRun {
            run: run.into(),
            closure,
        },
        Actor::System,
        T,
    )
}

#[test]
fn a_finished_run_becomes_done_merged() {
    let mut w = with_run(finished_run());
    assert_eq!(
        w.runs_to_watch(),
        vec![("BAU-138".to_string(), PrKey::new("acme/app", 7))]
    );
    let applied = close(&mut w, "BAU-138", Closure::Merged).unwrap();
    assert_eq!(applied.logged, 1);
    assert!(
        first_text(&w, &applied).contains("gemergt"),
        "{}",
        first_text(&w, &applied)
    );
    let run = w.run("BAU-138").unwrap();
    assert_eq!(run.closure, Some(Closure::Merged));
    assert!(run.is_closed() && run.is_done());
    assert_eq!(
        Closure::Merged.label_msg().render(Lang::De),
        "Erledigt (gemergt)"
    );
    assert!(
        w.runs_to_watch().is_empty(),
        "a closed run is not asked again"
    );
}

#[test]
fn a_closed_pull_request_gets_its_own_state() {
    let mut w = with_run(finished_run());
    close(&mut w, "BAU-138", Closure::Closed).unwrap();
    assert_eq!(w.runs[0].closure, Some(Closure::Closed));
    assert_eq!(
        Closure::Closed.label_msg().render(Lang::De),
        "Erledigt (geschlossen)"
    );
}

#[test]
fn closing_twice_or_an_unknown_run_does_nothing() {
    let mut w = with_run(finished_run());
    close(&mut w, "BAU-138", Closure::Merged).unwrap();
    let before = w.clone();
    assert_eq!(
        close(&mut w, "BAU-138", Closure::Closed),
        Err(ActionError::NothingToDo)
    );
    assert_eq!(
        close(&mut w, "nope", Closure::Closed),
        Err(ActionError::NothingToDo)
    );
    assert_eq!(w, before);
}

#[test]
fn only_the_system_may_close_a_run() {
    let mut w = with_run(finished_run());
    let action = Action::CloseAgentRun {
        run: "BAU-138".into(),
        closure: Closure::Merged,
    };
    for actor in [Actor::User, Actor::Agent] {
        assert_eq!(
            w.apply(action.clone(), actor, T),
            Err(ActionError::SystemOnly)
        );
    }
    assert!(!w.runs[0].is_closed());
}

#[test]
fn working_runs_and_runs_without_a_pull_request_stay_untouched() {
    let mut running = finished_run();
    running.state = RunState::Running;
    let mut no_pr = finished_run();
    no_pr.pull_request = None;
    let mut no_repo = finished_run();
    no_repo.repo.clear();
    for run in [running, no_pr, no_repo] {
        let mut w = with_run(run);
        assert!(w.runs_to_watch().is_empty());
        assert_eq!(
            close(&mut w, "BAU-138", Closure::Merged),
            Err(ActionError::NothingToDo)
        );
    }
}

#[test]
fn a_paused_run_is_ended_when_its_pull_request_is_merged() {
    let mut run = finished_run();
    run.state = RunState::Paused;
    let mut w = with_run(run);
    close(&mut w, "BAU-138", Closure::Merged).unwrap();
    let run = &w.runs[0];
    assert!(run.is_idle() && run.is_closed());
    assert!(!run.is_active());
}

#[test]
fn a_follow_up_job_is_tied_to_the_pull_request_it_works_on() {
    let run = AgentRun::followup(
        RunKind::ReviewPr {
            review: PrKey::new("acme/app", 91),
        },
        "pr-91",
        "Review",
        "claude/x",
    );
    assert_eq!(run.related_pr(), Some(PrKey::new("acme/app", 91)));
    let mut w = with_run(run);
    let key = w.runs[0].key();
    close(&mut w, &key, Closure::Closed).unwrap();
}

#[test]
fn a_new_start_clears_the_state_and_old_data_still_loads() {
    let mut run = finished_run();
    run.closure = Some(Closure::Merged);
    run.start(AgentOptions::default());
    assert_eq!(run.closure, None);
    let text = serde_json::to_string(&finished_run()).unwrap();
    assert!(!text.contains("closure"), "{text}");
    let back: AgentRun = serde_json::from_str(&text).unwrap();
    assert_eq!(back.closure, None);
    let mut closed = finished_run();
    closed.closure = Some(Closure::Closed);
    let back: AgentRun = serde_json::from_str(&serde_json::to_string(&closed).unwrap()).unwrap();
    assert_eq!(back.closure, Some(Closure::Closed));
}
