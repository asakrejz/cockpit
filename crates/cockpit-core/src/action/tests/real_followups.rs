use super::*;
use crate::agent::{FollowupTask, PullRequestRef};

/// Review #85 on an agent branch with a red check.
fn ws85() -> Workspace {
    let mut w = ws();
    name_pr85(&mut w, "claude/BAU-127-list-separator", "List: Trennlinien");
    assert!(
        w.review(85)
            .unwrap()
            .check_runs
            .iter()
            .any(|c| c.status == Checks::Failed)
    );
    w
}

/// Brings BAU-142 to a finished real run.
fn done_ws() -> Workspace {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    report(
        &mut w,
        "BAU-142",
        AgentReport::Started {
            detail: "docker".into(),
        },
    )
    .unwrap();
    report(
        &mut w,
        "BAU-142",
        AgentReport::Files {
            files: vec!["src/ring.rs".into()],
        },
    )
    .unwrap();
    report(
        &mut w,
        "BAU-142",
        AgentReport::Done {
            summary: "Fokus-Ring ergänzt.".into(),
        },
    )
    .unwrap();
    w
}

fn pull(number: u32, existed: bool) -> Option<PullRequestRef> {
    Some(PullRequestRef {
        number,
        url: format!("https://github.example/o/r/pull/{number}"),
        existed,
    })
}

fn create(w: &mut Workspace, pull: Option<PullRequestRef>) -> Result<Applied, ActionError> {
    w.apply(
        Action::CreatePullRequest {
            run: "BAU-142".into(),
            title: "BAU-142: Fokus-Ring".into(),
            body: "Text".into(),
            pull,
        },
        Actor::User,
        T,
    )
}

// ---------- Follow-up jobs of the real agent ----------

#[test]
fn review_threads_start_a_followup_on_the_branch_of_the_pull_request() {
    let mut w = ws85();
    ok(
        &mut w,
        Action::AddressReviewThreads {
            review: 85,
            options: direct(),
        },
        1,
    );
    let run = w.runs.iter().find(|r| r.key() == "threads-85").unwrap();
    assert!(run.is_followup() && run.is_running());
    assert_eq!(run.branch(), "claude/BAU-127-list-separator");
    assert_eq!(
        run.kind,
        RunKind::Followup {
            review: 85,
            task: FollowupTask::ReviewThreads
        }
    );
    assert!(run.kind.runs_in_container());
    // A second job on the same pull request has to wait.
    fails(
        &mut w,
        Action::FixFailedChecks {
            review: 85,
            options: direct(),
        },
        ActionError::Agent(AgentError::AlreadyRunning),
    );
}

#[test]
fn failed_checks_are_named_in_the_run() {
    let mut w = ws85();
    ok(
        &mut w,
        Action::FixFailedChecks {
            review: 85,
            options: direct(),
        },
        1,
    );
    let run = w.runs.iter().find(|r| r.key() == "fixes-85").unwrap();
    let RunKind::Followup {
        task: FollowupTask::FailedChecks { checks },
        ..
    } = &run.kind
    else {
        panic!("falsche Art: {:?}", run.kind);
    };
    assert!(!checks.is_empty());
    assert!(
        w.review(85)
            .unwrap()
            .check_runs
            .iter()
            .filter(|c| checks.contains(&c.name))
            .all(|c| c.status == Checks::Failed)
    );
}

#[test]
fn followups_need_an_agent_branch_a_red_check_and_an_open_review() {
    // Branch of a person, not of the agent.
    let mut w = ws();
    fails(
        &mut w,
        Action::AddressReviewThreads {
            review: 85,
            options: direct(),
        },
        ActionError::Agent(AgentError::NotAgentBranch),
    );
    // No red check.
    let mut w = ws85();
    for r in w.reviews.iter_mut().filter(|r| r.number == 85) {
        for c in &mut r.check_runs {
            c.status = Checks::Passed;
        }
    }
    fails(
        &mut w,
        Action::FixFailedChecks {
            review: 85,
            options: direct(),
        },
        ActionError::Agent(AgentError::CheckNotFailed),
    );
    fails(
        &mut w,
        Action::AddressReviewThreads {
            review: 999,
            options: direct(),
        },
        ActionError::UnknownReview(999),
    );
}

#[test]
fn only_you_may_commission_followups_and_pull_requests() {
    let mut w = ws85();
    for actor in [Actor::Agent, Actor::System] {
        let before = w.clone();
        for a in [
            Action::AddressReviewThreads {
                review: 85,
                options: direct(),
            },
            Action::FixFailedChecks {
                review: 85,
                options: direct(),
            },
            Action::CreatePullRequest {
                run: "BAU-142".into(),
                title: "t".into(),
                body: String::new(),
                pull: pull(1, false),
            },
        ] {
            assert_eq!(w.apply(a, actor, T), Err(ActionError::UserOnly(actor)));
        }
        assert_eq!(w, before);
    }
}

#[test]
fn a_real_followup_finishes_with_a_note_and_no_new_branch_on_a_ticket() {
    let mut w = ws85();
    ok(
        &mut w,
        Action::AddressReviewThreads {
            review: 85,
            options: direct(),
        },
        1,
    );
    report(
        &mut w,
        "threads-85",
        AgentReport::Started {
            detail: "docker".into(),
        },
    )
    .unwrap();
    report(
        &mut w,
        "threads-85",
        AgentReport::Done {
            summary: "Zwei Kommentare umgesetzt.".into(),
        },
    )
    .unwrap();
    let run = w.runs.iter().find(|r| r.key() == "threads-85").unwrap();
    assert!(run.is_done());
    let item = w
        .items
        .iter()
        .find(|i| i.pr.as_ref().is_some_and(|p| p.number == 85))
        .unwrap();
    assert!(item.agent_note.as_deref().unwrap().contains("gepusht"));
    // Afterwards the next job may start.
    ok(
        &mut w,
        Action::FixFailedChecks {
            review: 85,
            options: direct(),
        },
        1,
    );
}

// ---------- Pull request for the branch of a finished run ----------

#[test]
fn a_finished_real_run_has_no_invented_pull_request_number() {
    let w = done_ws();
    let run = w.run("BAU-142").unwrap();
    assert!(run.is_done() && run.real);
    assert_eq!(run.pr_number, None);
    assert_eq!(run.pull_request, None);
}

#[test]
fn the_draft_names_the_ticket_and_the_summary() {
    let w = done_ws();
    let d = w.pull_request_draft("BAU-142").unwrap();
    assert!(d.title.starts_with("BAU-142: "));
    assert!(d.body.contains("Fokus-Ring ergänzt."));
    assert!(d.body.contains("src/ring.rs"));
    assert_eq!(d.head, "claude/BAU-142-toggle-fokus-ring-fehlt");
    assert_eq!(d.base, "main");
    assert_eq!(w.pull_request_draft("gibt-es-nicht"), None);
}

#[test]
fn creating_the_pull_request_is_booked_once() {
    let mut w = done_ws();
    let before = w.log.len();
    create(&mut w, pull(130, false)).unwrap();
    assert_eq!(w.log.len(), before + 1);
    let last = w.log.last().unwrap();
    assert_eq!(last.actor, Actor::User);
    assert_eq!(last.target, Some(Target::Run("BAU-142".into())));
    assert!(last.text.contains("#130") && last.text.contains("angelegt"));
    let run = w.run("BAU-142").unwrap();
    assert_eq!(run.pr_number, Some(130));
    assert_eq!(run.pull_request.as_ref().unwrap().number, 130);
    // No duplicate.
    let snapshot = w.clone();
    assert_eq!(
        create(&mut w, pull(131, false)),
        Err(ActionError::PullRequestExists(130))
    );
    assert_eq!(w, snapshot);
}

#[test]
fn an_existing_pull_request_is_linked_not_created() {
    let mut w = done_ws();
    create(&mut w, pull(77, true)).unwrap();
    let last = w.log.last().unwrap();
    assert!(last.text.contains("#77") && last.text.contains("schon"));
    assert!(w.run("BAU-142").unwrap().pull_request.unwrap().existed);
}

#[test]
fn the_pull_request_needs_a_finished_real_run_a_title_and_the_answer_of_github() {
    // Not started at all.
    let mut w = ws();
    assert_eq!(
        create(&mut w, pull(1, false)),
        Err(ActionError::NothingToDo)
    );
    // Running, not done.
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    assert_eq!(
        create(&mut w, pull(1, false)),
        Err(ActionError::PullRequestNotReady)
    );
    // Without the answer of GitHub or without a title nothing is booked.
    let mut w = done_ws();
    let snapshot = w.clone();
    assert_eq!(create(&mut w, None), Err(ActionError::InvalidInput));
    let blank = Action::CreatePullRequest {
        run: "BAU-142".into(),
        title: "  ".into(),
        body: String::new(),
        pull: pull(1, false),
    };
    assert_eq!(
        w.apply(blank, Actor::User, T),
        Err(ActionError::InvalidInput)
    );
    assert_eq!(w, snapshot);
}

#[test]
fn the_new_fields_are_backward_compatible() {
    // A run saved before this feature has no `pull_request` and loads.
    let w = done_ws();
    let json = serde_json::to_value(w.run("BAU-142").unwrap()).unwrap();
    assert!(
        json.get("pull_request").is_none(),
        "leer wird nicht geschrieben"
    );
    let back: AgentRun = serde_json::from_value(json).unwrap();
    assert_eq!(back.pull_request, None);
    // The actions travel as JSON; `pull` may be missing.
    let a: Action =
        serde_json::from_str(r#"{"CreatePullRequest":{"run":"BAU-142","title":"t","body":"b"}}"#)
            .unwrap();
    assert!(matches!(a, Action::CreatePullRequest { pull: None, .. }));
    let a = Action::AddressReviewThreads {
        review: 85,
        options: direct(),
    };
    let text = serde_json::to_string(&a).unwrap();
    assert_eq!(serde_json::from_str::<Action>(&text).unwrap(), a);
    // And the persisted workspace with a finished pull request survives a round trip.
    let mut w = done_ws();
    create(&mut w, pull(130, false)).unwrap();
    let back = persist::load(&persist::save(&w)).unwrap();
    assert_eq!(
        back.run("BAU-142").unwrap().pull_request,
        w.run("BAU-142").unwrap().pull_request
    );
}
