//! Wire/persistence compatibility: the saved JSON must stay byte-identical across renames
//! of Rust identifiers. `wire_*.json` are the current snapshots (they write `"User"` where
//! the older versions wrote `"Du"`); `legacy_*.json` are the snapshots written before the English
//! renaming and must keep loading (the old names are read through `serde(alias)`).
//! `legacy_sync_state_problem.json` is a sync state in which one problem is stored as a plain text
//! and one as a message; both must load.
//! `legacy_simulated_agent_workspace.json` is a state saved while the simulated agent still
//! existed: runs with the removed fields and kinds, a diagnosis, sample suggestions, checks that
//! count ticks. It must load, and saving it again must leave those parts out.
//! `legacy_removed_fields_workspace.json` is a state that still holds what was removed from the
//! data: the email and Slack channels of the notification settings and the `resolved` mark of line
//! comments. It must load, and saving it again must leave those parts out.
//! Regenerate `wire_*` with `BLESS=1 cargo test -p cockpit-core --test wire_compat` only on purpose.

use cockpit_core::{
    Action, Actor, AgentOptions, AgentReport, Channel, FollowupTask, NotifyReason, Problem, Rule,
    RunKind, RunState, Source, SyncState, TicketStatus, Workspace, demo, persist,
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
        persist::save_with_revision(&demo::workspace(), 3),
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
    for name in ["wire_demo_workspace.json", "wire_busy_workspace.json"] {
        let text = read(name);
        let (ws, revision) = persist::load_with_revision(&text).expect("snapshot loads");
        assert!(
            persist::save_with_revision(&ws, revision) == text,
            "{name}: load + save changed the bytes"
        );
    }
}

#[test]
fn legacy_snapshots_still_load_and_resave_in_the_new_names() {
    for legacy in ["legacy_demo_workspace.json", "legacy_busy_workspace.json"] {
        let text = read(legacy);
        let (ws, revision) = persist::load_with_revision(&text).expect("old snapshot loads");
        let saved = persist::save_with_revision(&ws, revision);
        assert!(
            !saved.contains("\"actor\":\"Du\""),
            "{legacy}: an old name is still written"
        );
        let (again, _) = persist::load_with_revision(&saved).expect("the saved state loads");
        assert_eq!(again, ws, "{legacy}");
        assert_eq!(persist::save_with_revision(&again, revision), saved);
    }
}

/// A state saved without a revision reads as revision 0.
#[test]
fn a_snapshot_without_revision_loads_as_revision_zero() {
    let text = read("legacy_demo_workspace.json");
    assert!(!text.contains("\"revision\""));
    let (_, revision) = persist::load_with_revision(&text).expect("old snapshot loads");
    assert_eq!(revision, 0);
}

/// A state with the removed notification channels (email, Slack) and the removed `resolved` mark
/// on line comments loads: the browser choices stay, the rest is ignored and not written again.
#[test]
fn a_state_with_removed_channels_and_marks_still_loads() {
    let text = read("legacy_removed_fields_workspace.json");
    // The fixture really holds what was removed.
    for old in [
        "\"email\":true",
        "\"slack\":true",
        "\"resolved\":\"review-87\"",
    ] {
        assert!(text.contains(old), "the fixture lost {old}");
    }

    let (ws, revision) = persist::load_with_revision(&text).expect("the old state loads");
    assert!(
        ws.notifications
            .enabled(NotifyReason::Mention, Channel::Browser)
    );
    assert!(
        !ws.notifications
            .enabled(NotifyReason::AgentFinished, Channel::Browser)
    );
    assert!(
        ws.notifications
            .enabled(NotifyReason::Assigned, Channel::Browser)
    );
    // Every comment is there; the own one goes out with the review again.
    let review = ws.review(91).unwrap();
    assert_eq!(review.line_comments.len(), 3);
    assert_eq!(review.comment_count(), review.approved() + 1);

    let saved = persist::save_with_revision(&ws, revision);
    for gone in ["\"email\"", "\"slack\"", "\"resolved\""] {
        assert!(!saved.contains(gone), "{gone} is still written");
    }
    let (again, _) = persist::load_with_revision(&saved).expect("the saved state loads");
    assert_eq!(again, ws);
    assert_eq!(persist::save_with_revision(&again, revision), saved);
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

/// A stored pull request key is read through the repo name rule: the names GitHub allows (dots,
/// underscores, upper case, a leading dot in the name) keep loading; a name no repo can have
/// does not.
#[test]
fn stored_pull_request_keys_with_usual_github_names_still_load() {
    for key in [
        "acme/app#3",
        "Acme/App.JS#4",
        "my-org/my_repo.name#5",
        "acme/.github#6",
        "a/b#7",
    ] {
        let loaded: cockpit_core::PrKey =
            serde_json::from_str(&format!("\"{key}\"")).unwrap_or_else(|e| panic!("{key}: {e}"));
        assert_eq!(loaded.to_string(), key);
    }
    for key in ["acme/..#1", "../x#1", "a--b/c#1", "acme/a/b#1", "a_b/c#1"] {
        assert!(
            serde_json::from_str::<cockpit_core::PrKey>(&format!("\"{key}\"")).is_err(),
            "{key}"
        );
    }
}

fn read(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}
