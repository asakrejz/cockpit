//! Wire/persistence compatibility: the saved JSON must stay byte-identical across renames
//! of Rust identifiers. `wire_*.json` are the current snapshots (they write `"User"` where
//! the older versions wrote `"Du"`); `legacy_*.json` are the snapshots written before the English
//! renaming and must keep loading (the old names are read through `serde(alias)`).
//! `wire_pre_repo_*.json` are the current snapshots as they were before several repos were
//! supported (no `repo` on reviews and pull request references); they must keep loading and
//! resaving byte-identically (empty repos are not written).
//! `legacy_sync_state_problem.json` is a sync state in which one problem is stored as a plain text
//! and one as a message; both must load.
//! `legacy_simulated_agent_workspace.json` is a state saved while the simulated agent still
//! existed: runs with the removed fields and kinds, a diagnosis, sample suggestions, checks that
//! count ticks. It must load, and saving it again must leave those parts out.
//! Regenerate `wire_*` with `BLESS=1 cargo test -p cockpit-core --test wire_compat` only on purpose.

use cockpit_core::{
    Action, Actor, AgentOptions, AgentReport, FollowupTask, Problem, Rule, RunKind, RunState,
    Source, SyncState, TicketStatus, Workspace, demo, persist,
};
use cockpit_i18n::{Lang, Msg};

const USER: Actor = Actor::User;

fn busy_workspace() -> Workspace {
    let mut ws = demo::workspace();
    let steps = [
        (
            Action::SetStatus {
                item: "bau142".into(),
                status: TicketStatus::Review,
            },
            USER,
        ),
        (
            Action::Snooze {
                item: "bau150".into(),
                until_ms: Some(1_800_000_000_000),
            },
            USER,
        ),
        (
            Action::SetRule {
                rule: Rule::Overdue,
                enabled: false,
            },
            USER,
        ),
        (
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: AgentOptions::default(),
            },
            USER,
        ),
        (
            Action::ReportAgent {
                run: "BAU-142".into(),
                report: AgentReport::Plan {
                    summary: "Fokus-Ring ergänzen".into(),
                    files: vec!["components.css".into()],
                    steps: vec!["Ändern".into()],
                },
            },
            Actor::Agent,
        ),
        (
            Action::ApprovePlan {
                ticket: "BAU-142".into(),
            },
            USER,
        ),
        (
            Action::ReportAgent {
                run: "BAU-142".into(),
                report: AgentReport::Step {
                    title: "Datei gelesen".into(),
                    text: "components.css".into(),
                    input_tokens: 8_200,
                    output_tokens: 900,
                    millis: 18_000,
                },
            },
            Actor::Agent,
        ),
    ];
    for (n, (action, actor)) in steps.into_iter().enumerate() {
        let _ = ws.apply(action, actor, 1_700_000_000_000 + n as u64);
    }
    ws
}

fn check(name: &str, actual: String) {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert!(
        actual == expected,
        "{name} differs from the stored snapshot"
    );
}

#[test]
fn demo_workspace_serializes_byte_identically() {
    check(
        "wire_demo_workspace.json",
        persist::save(&demo::workspace()),
    );
}

#[test]
fn workspace_with_actions_and_log_serializes_byte_identically() {
    check(
        "wire_busy_workspace.json",
        persist::save_with_revision(&busy_workspace(), 7),
    );
}

#[test]
fn current_snapshots_load_and_resave_identically() {
    for name in [
        "wire_demo_workspace.json",
        "wire_busy_workspace.json",
        "wire_pre_repo_demo_workspace.json",
        "wire_pre_repo_busy_workspace.json",
    ] {
        let text = read(name);
        let (ws, revision) = persist::load_with_revision(&text).expect("snapshot loads");
        assert!(
            resave(&ws, revision, &text) == text,
            "{name}: load + save changed the bytes"
        );
    }
}

#[test]
fn legacy_snapshots_still_load_and_resave_in_the_new_names() {
    for (legacy, current) in [
        (
            "legacy_demo_workspace.json",
            "wire_pre_repo_demo_workspace.json",
        ),
        (
            "legacy_busy_workspace.json",
            "wire_pre_repo_busy_workspace.json",
        ),
    ] {
        let text = read(legacy);
        let (ws, revision) = persist::load_with_revision(&text).expect("old snapshot loads");
        // Resaving must give the stored `wire_pre_repo_*` snapshot.
        check(current, resave(&ws, revision, &text));
    }
}

/// A state from before several repos were supported gets its repos from the migration and
/// then behaves like the current one.
#[test]
fn pre_repo_snapshots_get_their_repos_from_the_migration() {
    let text = read("wire_pre_repo_demo_workspace.json");
    let mut ws = persist::load(&text).expect("loads");
    assert!(ws.reviews.iter().all(|r| r.repo.is_empty()));
    assert!(ws.fill_repos(Some(cockpit_core::demo::DEMO_REPO)));
    assert!(ws.reviews.iter().all(|r| !r.repo.is_empty()));
    // Idempotent.
    assert!(!ws.fill_repos(Some(cockpit_core::demo::DEMO_REPO)));
    // The tokens repo's pull request keeps the repo of its entry instead of the default,
    // and a ticket's copy of #91 takes the default.
    let tokens = ws.item("pr12").unwrap().pr.as_ref().unwrap();
    assert_eq!(tokens.repo, "acme/design-tokens");
    assert_eq!(ws.review(91).unwrap().repo, cockpit_core::demo::DEMO_REPO);
    assert_eq!(
        ws.item("bau138").unwrap().pr.as_ref().unwrap().repo,
        cockpit_core::demo::DEMO_REPO
    );
    // Saving and loading the migrated state keeps it.
    assert_eq!(persist::load(&persist::save(&ws)).unwrap(), ws);
}

/// A state from the time of the simulated agent loads: the removed fields are ignored, the two
/// removed kinds of run become the follow-up jobs of the same purpose, and a run that waited for
/// the decision about a fix is ready again. Saving leaves all of that out.
#[test]
fn a_state_with_the_simulated_agent_still_loads() {
    let text = read("legacy_simulated_agent_workspace.json");
    // The fixture really holds what was removed.
    for old in [
        "\"real\":",
        "\"AwaitingFix\"",
        "\"diagnosis\":{",
        "\"AddressReview\":{",
        "\"Diagnose\":{",
        "\"demo_dropped\"",
        "\"remaining\":",
        "\"rerun\":",
        "\"origin\":\"Agent\"",
    ] {
        assert!(text.contains(old), "the fixture lost {old}");
    }

    let (ws, revision) = persist::load_with_revision(&text).expect("the old state loads");
    assert_eq!(revision, 5);
    assert_eq!(ws.runs.len(), 5);

    // The finished sample run and the questions of the simulation are plain runs now.
    assert!(ws.run("BAU-138").unwrap().is_done());
    assert_eq!(ws.run("BAU-142").unwrap().state, RunState::AwaitingAnswer);
    assert!(ws.run("BAU-150").unwrap().is_running());

    // The simulated follow-up jobs read as the follow-up jobs of the real agent.
    let review = cockpit_core::PrKey::new(demo::DEMO_REPO, 85);
    let diagnose = ws
        .runs
        .iter()
        .find(|r| r.ticket_key == "acme/app#85")
        .unwrap();
    assert_eq!(
        diagnose.kind,
        RunKind::Followup {
            review: review.clone(),
            task: FollowupTask::FailedChecks {
                checks: vec!["Lint".into()]
            }
        }
    );
    assert_eq!(diagnose.key(), "fixes-acme~app-85");
    assert_eq!(diagnose.state, RunState::Idle, "the fix decision is gone");
    let address = ws.run("threads-acme~app-91").unwrap();
    assert_eq!(
        address.kind,
        RunKind::Followup {
            review: cockpit_core::PrKey::new(demo::DEMO_REPO, 91),
            task: FollowupTask::ReviewThreads
        }
    );

    // Suggestions keep their text, with or without a position.
    let r85 = ws.review(85).unwrap();
    assert_eq!(r85.suggestions.len(), 1);
    assert!(r85.suggestions[0].path.is_empty());
    let r91 = ws.review(91).unwrap();
    assert_eq!(r91.suggestions.len(), 1);
    assert_eq!(r91.suggestions[0].path, "controls.rs");
    // Checks that counted ticks are plain checks now.
    assert!(
        r85.check_runs
            .iter()
            .any(|c| c.status == cockpit_core::Checks::Running)
    );

    // Log entries of the simulation carry codes that are gone: they show the text saved with them.
    let stale: Vec<_> = ws
        .log
        .iter()
        .filter(|e| e.msg.as_ref().is_some_and(|m| !m.is_known()))
        .collect();
    assert!(
        !stale.is_empty(),
        "the fixture holds log entries with removed codes"
    );
    for entry in stale {
        for lang in cockpit_i18n::Lang::ALL {
            assert_eq!(entry.text_in(lang), entry.text);
        }
    }
    assert!(
        ws.runs
            .iter()
            .flat_map(|r| &r.messages)
            .filter(|m| m.msg.as_ref().is_some_and(|m| !m.is_known()))
            .all(|m| m.text_in(cockpit_i18n::Lang::En) == m.text)
    );

    // Saving leaves the removed parts out, and the saved state is stable.
    let saved = persist::save_with_revision(&ws, revision);
    for gone in [
        "\"real\":",
        "AwaitingFix",
        "\"diagnosis\"",
        "\"AddressReview\"",
        "\"Diagnose\"",
        "demo_dropped",
        "\"remaining\"",
        "\"rerun\"",
        "\"origin\"",
    ] {
        assert!(!saved.contains(gone), "{gone} is still written");
    }
    let (again, _) = persist::load_with_revision(&saved).expect("the saved state loads");
    assert_eq!(again, ws);
    assert_eq!(persist::save_with_revision(&again, revision), saved);
}

#[test]
fn the_busy_legacy_snapshot_really_uses_the_old_name() {
    assert!(read("legacy_busy_workspace.json").contains("\"actor\":\"Du\""));
    assert!(!read("wire_busy_workspace.json").contains("\"actor\":\"Du\""));
}

/// A problem that was stored as a plain text still loads, as a raw message that reads the same in
/// every language; a stored message keeps its code and renders in the chosen language.
#[test]
fn a_stored_problem_loads_as_text_or_as_message() {
    let state: SyncState = serde_json::from_str(&read("legacy_sync_state_problem.json"))
        .expect("old sync state loads");

    let jira = state.state_of(Source::Jira).unwrap();
    let text = "Unexpected status from the service: 502";
    assert_eq!(jira.problem, Some(Problem::Other(Msg::raw(text))));
    for lang in Lang::ALL {
        assert_eq!(jira.problem.as_ref().unwrap().msg().render(lang), text);
    }

    let github = state.state_of(Source::GitHub).unwrap();
    let message = github.problem.as_ref().unwrap().msg();
    assert_eq!(message.code, "integrations.server_error");
    assert!(message.render(Lang::En).contains("503"));
    assert!(message.render(Lang::De).contains("503"));
    assert_ne!(message.render(Lang::En), message.render(Lang::De));

    // Saving writes the message form, which loads again unchanged.
    let saved = serde_json::to_string(&state).unwrap();
    assert!(!saved.contains("\"Other\":\"Unexpected"), "{saved}");
    assert_eq!(serde_json::from_str::<SyncState>(&saved).unwrap(), state);
}

fn read(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

fn resave(ws: &Workspace, revision: u64, text: &str) -> String {
    if text.contains("\"revision\"") {
        persist::save_with_revision(ws, revision)
    } else {
        persist::save(ws)
    }
}
