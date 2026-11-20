use super::*;
use crate::model::{Filter, ReasonKind, Scope, Tone};
use crate::notify::NotifySettings;
use crate::persist;
use crate::review::{CHECK_STEPS, Decision};

const T: u64 = 1_700_000_000_000;

fn ws() -> Workspace {
    demo::workspace()
}

fn direct() -> AgentOptions {
    AgentOptions {
        plan_first: false,
        ..Default::default()
    }
}

/// Applies and expects exactly `n` new entries.
fn ok(w: &mut Workspace, action: Action, n: usize) -> Applied {
    let before = w.log.len();
    let applied = w.apply(action.clone(), Actor::User, T).unwrap();
    assert_eq!(applied.logged, n, "{action:?}");
    assert_eq!(w.log.len(), before + n, "{action:?}");
    applied
}

fn fails(w: &mut Workspace, action: Action, error: ActionError) {
    let before = w.clone();
    assert_eq!(w.apply(action, Actor::User, T), Err(error));
    assert_eq!(*w, before, "an error changes nothing and logs nothing");
}

/// Brings BAU-142 up to the agent's follow-up question.
fn asking_ws() -> Workspace {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    ok(&mut w, Action::AdvanceAgents, 1);
    ok(&mut w, Action::AdvanceAgents, 1);
    ok(&mut w, Action::AdvanceAgents, 1);
    assert!(w.run("BAU-142").unwrap().is_asking());
    w
}

// ---------- Follow-up jobs ----------

fn run_to_end(w: &mut Workspace) {
    for _ in 0..STEPS * 2 {
        // The test answers follow-up questions with the first suggestion.
        let asking = w
            .runs
            .iter()
            .find(|r| r.is_asking())
            .map(|r| (r.key(), r.question.as_ref().unwrap().suggestions[0].clone()));
        if let Some((run, text)) = asking {
            w.apply(Action::AnswerAgent { run, text }, Actor::User, T)
                .unwrap();
        }
        if w.running_agents() == 0 {
            break;
        }
        w.apply(Action::AdvanceAgents, Actor::Agent, T).unwrap();
    }
}

fn address(review: u32, ids: &[&str], options: AgentOptions) -> Action {
    Action::AddressReview {
        review: review.into(),
        comments: ids.iter().map(|s| s.to_string()).collect(),
        options,
    }
}

fn investigate_85(w: &mut Workspace, check: &str) {
    ok(
        w,
        Action::InvestigateCheck {
            review: 85.into(),
            check: check.into(),
        },
        1,
    );
}

use crate::agent::RunState;

// ---------- Costs, limits, rules ----------

fn advance(w: &mut Workspace, n: u64) {
    for i in 0..n {
        let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T + i);
    }
}

fn limits(f: impl FnOnce(&mut AgentLimits)) -> Action {
    let mut l = AgentLimits::default();
    f(&mut l);
    Action::SetAgentLimits { limits: l }
}

fn start(w: &mut Workspace, key: &str) {
    ok(
        w,
        Action::StartAgent {
            ticket: key.into(),
            options: direct(),
        },
        1,
    );
}

fn rules(f: impl FnOnce(&mut RepoRules)) -> Action {
    let mut r = RepoRules::default();
    f(&mut r);
    Action::SetRepoRules {
        repo: crate::rules::DEMO_REPO.into(),
        rules: r,
    }
}

// ---------- Ticket and pull request ----------

/// Links a ticket and a pull request the only way there is: the branch names the ticket key
/// and the automation links them. `n` is the number of log entries it must write.
fn link(w: &mut Workspace, item: &str, pr: u32, n: usize) -> Applied {
    let key = w.item(item).unwrap().key.to_lowercase();
    let review = w.reviews.iter_mut().find(|r| r.number == pr).unwrap();
    review.branch = format!("feature/{key}-x");
    review.ticket_key = None;
    ok(w, Action::AutoLinkPrs, n)
}

fn status(w: &Workspace, id: &str) -> TicketStatus {
    w.item(id).unwrap().status
}

fn name_pr85(w: &mut Workspace, branch: &str, title: &str) {
    let review = w.reviews.iter_mut().find(|r| r.number == 85).unwrap();
    review.branch = branch.into();
    review.title = title.into();
    review.ticket_key = None;
}

// ---------- Reports of the real agent ----------

fn report(w: &mut Workspace, run: &str, report: AgentReport) -> Result<Applied, ActionError> {
    w.apply(
        Action::ReportAgent {
            run: run.into(),
            report,
        },
        Actor::Agent,
        T,
    )
}

// ---------- Merging, checks, change suggestions ----------

/// #87 is ready to merge: all suggestions decided, review sent.
fn ready_87(w: &mut Workspace) {
    for id in ["a", "b", "c"] {
        ok(
            w,
            Action::Decide {
                review: 87.into(),
                suggestion: id.into(),
                decision: Some(Decision::Declined),
            },
            1,
        );
    }
    ok(
        w,
        Action::SendReview {
            review: 87.into(),
            comment: String::new(),
        },
        1,
    );
}

fn merge(review: u32, method: MergeMethod) -> Action {
    Action::MergePr {
        review: review.into(),
        method,
    }
}

// ---------- Edit tickets ----------

fn edit(item: &str, title: &str, description: &str) -> Action {
    Action::EditTicket {
        item: item.into(),
        title: title.into(),
        description: description.into(),
    }
}

fn assign(item: &str, person: Person) -> Action {
    Action::AssignTicket {
        item: item.into(),
        person,
    }
}

fn lena() -> Person {
    Person::new("Lena", "LK")
}

mod agent_review;
mod basics;
mod closed_runs;
mod follow_ups;
mod inbox_filter;
mod limits;
mod multi_repo;
mod pull_requests;
mod real_agent;
mod real_followups;
mod repos;
mod review;
mod review_filter;
mod status_sync;
mod tickets;

#[test]
fn the_language_is_chosen_and_stored_in_the_workspace() {
    use cockpit_i18n::Lang;
    let mut w = Workspace::default();
    assert_eq!(w.language, None);
    ok(&mut w, Action::SetLanguage { language: Lang::De }, 1);
    assert_eq!(w.language, Some(Lang::De));
    assert!(matches!(
        w.apply(Action::SetLanguage { language: Lang::De }, Actor::User, T),
        Err(ActionError::Unchanged)
    ));
    // Old saved states without the field still load.
    let saved = serde_json::to_string(&Workspace::default()).unwrap();
    assert!(!saved.contains("language"));
    let loaded: Workspace = serde_json::from_str(&saved).unwrap();
    assert_eq!(loaded.language, None);
}

#[test]
fn a_coded_log_entry_reads_in_both_languages_and_keeps_the_german_text() {
    use cockpit_i18n::Lang;
    let mut w = Workspace::default();
    ok(&mut w, Action::SetLanguage { language: Lang::En }, 1);
    let entry = w.log.last().unwrap();
    assert_eq!(entry.text, "Sprache der Oberfläche: English");
    assert_eq!(entry.text_in(Lang::En), "Interface language: English");
    assert_eq!(entry.text_in(Lang::De), "Sprache der Oberfläche: English");
    // An old entry without a code is shown as it is.
    let old = LogEntry {
        id: 1,
        at_ms: 0,
        actor: Actor::User,
        target: None,
        text: "Etwas passiert".into(),
        msg: None,
    };
    assert_eq!(old.text_in(Lang::En), "Etwas passiert");
}
