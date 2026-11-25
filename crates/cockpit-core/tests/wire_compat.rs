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
//! `legacy_german_strings_workspace.json` is a state in which German text is stored in the data: the
//! reason line and the time of entries and comments as plain text, me as the name `Du`, nobody as
//! a German name. It must load and show what it stored; saving it again writes me and nobody as
//! roles.
//! `legacy_plain_text_twins_workspace.json` is a state in which every message also carries its German
//! plain text and in which the run keeps its reason, summary and refusals as plain text. It must
//! load and show what it stored.
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

/// Entries and comments that carry German text instead of a message and a timestamp still load
/// and show their text; me and nobody read as the roles; saving writes the roles, and the new
/// fields (`reason_msg`, `updated_ms`, `created_ms`) stay out until a sync fills them.
#[test]
fn a_state_with_german_strings_in_the_data_still_loads() {
    let text = read("legacy_german_strings_workspace.json");
    for old in [
        "\"reason_text\":\"Dir zugewiesen\"",
        "\"time\":\"vor 5 Minuten\"",
        "\"name\":\"Du\"",
        "\"name\":\"Nicht zugewiesen\"",
    ] {
        assert!(text.contains(old), "the fixture lost {old}");
    }

    let (ws, revision) = persist::load_with_revision(&text).expect("the old state loads");
    let ticket = ws.item("bau7").unwrap();
    // The stored text is what the row shows, in either language, until a sync replaces it.
    assert_eq!(ticket.reason_msg, None);
    for lang in Lang::ALL {
        assert_eq!(ticket.reason_line().render(lang), "Dir zugewiesen");
    }
    assert_eq!(ticket.time, "vor 5 Minuten");
    // Me is the role, in the language of the reader; the stored initials are not kept.
    assert!(ticket.owner.me && ticket.owner.name.is_empty() && ticket.owner.initials.is_empty());
    assert_eq!(ticket.owner.name_in(Lang::De), "Du");
    assert_eq!(ticket.owner.name_in(Lang::En), "You");
    assert_eq!(ticket.owner.initials_in(Lang::En), "Y");
    assert!(ticket.comments[1].author.me && ticket.comments[1].own);
    assert_eq!(ticket.comments[1].time, "gerade eben");
    assert_eq!(ticket.comments[1].created_ms, None);
    // The mention marker and the plain name fields keep their stored word.
    assert!(cockpit_core::attention::mentions_me(
        &ticket.comments[0].text
    ));
    assert_eq!(ticket.reporter.as_deref(), Some(cockpit_core::ME_MARK));
    assert_eq!(ticket.co_assignees, [cockpit_core::ME_MARK, "Lena"]);
    assert_eq!(ticket.updated_ms, Some(1_700_000_000_000));

    let unassigned = ws.item("bau8").unwrap();
    assert!(unassigned.owner.nobody && unassigned.owner.name.is_empty());
    assert_eq!(unassigned.owner.name_in(Lang::En), "Unassigned");
    assert_eq!(unassigned.owner.name_in(Lang::De), "Nicht zugewiesen");
    assert_eq!(unassigned.reason_line().render(Lang::De), "Beobachtet");
    // Search still finds the stored reason line.
    assert!(cockpit_core::search::matches_query(
        unassigned,
        "beobachtet"
    ));

    // The owner filters work on the roles.
    assert_eq!(ws.owners().first(), Some(&cockpit_core::Person::me()));
    assert!(ws.owners().contains(&cockpit_core::Person::nobody()));

    let saved = persist::save_with_revision(&ws, revision);
    for gone in [
        "\"name\":\"Du\"",
        "Nicht zugewiesen\",\"initials",
        "\"initials\":\"XY\"",
    ] {
        assert!(!saved.contains(gone), "{gone} is still written");
    }
    assert!(saved.contains("\"me\":true") && saved.contains("\"nobody\":true"));
    // The stored texts stay until a sync replaces the entries.
    assert!(saved.contains("\"reason_text\":\"Dir zugewiesen\""));
    assert!(!saved.contains("reason_msg") && !saved.contains("created_ms"));
    let (again, _) = persist::load_with_revision(&saved).expect("the saved state loads");
    assert_eq!(again, ws);
    assert_eq!(persist::save_with_revision(&again, revision), saved);
}

/// New data carries the message and the timestamps and no German plain text of its own.
#[test]
fn new_entries_store_a_message_and_timestamps_not_text() {
    let mut ws = Workspace::default();
    let (mut ticket, mut comment) = (
        demo::workspace().item("bau150").unwrap().clone(),
        cockpit_core::Comment::at(cockpit_core::Person::me(), 1_700_000_000_000, "Hallo"),
    );
    ticket.reason_text.clear();
    ticket.time.clear();
    ticket.set_reason(
        cockpit_core::ReasonKind::Assigned,
        cockpit_i18n::msg!(ReasonAssigned),
    );
    ticket.updated_ms = Some(1_700_000_000_000);
    comment.own = true;
    ticket.comments = vec![comment];
    ws.items.push(ticket);
    let saved = persist::save_with_revision(&ws, 1);
    assert!(saved.contains("\"reason_msg\":{\"code\":\"reason.assigned\"}"));
    assert!(saved.contains("\"created_ms\":1700000000000"));
    assert!(!saved.contains("\"reason_text\"") && !saved.contains("\"time\""));
    let (again, _) = persist::load_with_revision(&saved).expect("the saved state loads");
    assert_eq!(again, ws);
}

/// A state saved while every message also carried its German plain text (log entries, messages of
/// a run, the note and the comments of an entry, the hints of the sync) and while the run kept
/// its reason, summary and refusals as plain text. It must load, show what it stored (the message
/// in the reader's language, the plain text where there is no message or its code is gone) and
/// save again without losing a text.
#[test]
fn a_state_with_plain_texts_next_to_its_messages_still_loads() {
    let text = read("legacy_plain_text_twins_workspace.json");
    for old in [
        "\"text\":\"Status von BAU-142 auf „Review“ gesetzt\"",
        "\"agent_note\":\"Der Agent arbeitet an der Umsetzung\"",
        "\"ended\":\"Zeitlimit des Containers überschritten\"",
        "\"summary\":\"Der Branch claude/BAU-142 ist gepusht (2 Dateien).\"",
        "\"reason\":\"Pfad verboten: tokens.css\"",
        "\"warnings\":[\"acme/web ist nicht sichtbar\"]",
        "\"text\":\"Etwas ist passiert\"",
    ] {
        assert!(text.contains(old), "the fixture lost {old}");
    }

    let (ws, revision) = persist::load_with_revision(&text).expect("the old state loads");

    // A log entry with a message reads in either language; one without, or with a code that is
    // gone, shows its stored text.
    let first = &ws.log[0];
    assert_eq!(
        first.text_in(Lang::De),
        "Status von BAU-142 auf „Review“ gesetzt"
    );
    assert_eq!(
        first.text_in(Lang::En),
        "Status of BAU-142 set to \"Review\""
    );
    let gone = ws
        .log
        .iter()
        .find(|e| e.text == "Schritt 3 abgeschlossen")
        .unwrap();
    assert!(!gone.msg.as_ref().unwrap().is_known());
    let plain = ws
        .log
        .iter()
        .find(|e| e.text == "Etwas ist passiert")
        .unwrap();
    assert!(plain.msg.is_none());
    for lang in Lang::ALL {
        assert_eq!(gone.text_in(lang), "Schritt 3 abgeschlossen");
        assert_eq!(plain.text_in(lang), "Etwas ist passiert");
    }

    // The messages of a run: the system's in the reader's language, the user's words as written.
    let run = ws.run("BAU-142").unwrap();
    assert_eq!(
        run.messages[0].text_in(Lang::En),
        "Job for BAU-142 accepted."
    );
    assert_eq!(
        run.messages[0].text_in(Lang::De),
        "Auftrag für BAU-142 angenommen."
    );
    let hint = run.messages.last().unwrap();
    assert_eq!(hint.text_in(Lang::En), "Bitte auch die Doku prüfen");
    assert!(
        run.messages
            .iter()
            .any(|m| m.text_in(Lang::En) == "Schritt 3 abgeschlossen")
    );

    // Reason, summary and refusal of the run were plain text: raw messages, the same everywhere.
    assert_eq!(
        run.ended,
        Some(Msg::raw("Zeitlimit des Containers überschritten"))
    );
    assert_eq!(
        run.summary,
        Some(Msg::raw(
            "Der Branch claude/BAU-142 ist gepusht (2 Dateien)."
        ))
    );
    assert_eq!(
        run.repos[0].reason,
        Some(Msg::raw("Pfad verboten: tokens.css"))
    );
    assert_eq!(ws.run("BAU-138").unwrap().ended, None);
    assert_eq!(ws.run("BAU-138").unwrap().summary, None);

    // The note of the agent: the message, or the stored text of an entry that has none.
    let working = ws.item("bau142").unwrap();
    assert_eq!(
        working.agent_note_in(Lang::En).as_deref(),
        Some("The agent is working on the implementation")
    );
    assert_eq!(
        working.agent_note_in(Lang::De).as_deref(),
        Some("Der Agent arbeitet an der Umsetzung")
    );
    let waiting = ws.item("pr87").unwrap();
    for lang in Lang::ALL {
        assert_eq!(
            waiting.agent_note_in(lang).as_deref(),
            Some("3 Kommentare vom Agenten warten auf deine Freigabe")
        );
    }

    // A review comment: the message with the verdict in front, the words stay the text.
    let comment = waiting.comments.last().unwrap();
    assert_eq!(
        comment.text_in(Lang::En),
        "Review: approved\nSieht gut aus."
    );
    assert!(comment.text.contains("Sieht gut aus."));

    // The hints of the sync: the messages in the reader's language, the plain texts of a state
    // saved earlier where there are no messages.
    let github = ws.sync.state_of(Source::GitHub).unwrap();
    assert_eq!(
        github.warning_lines(Lang::En),
        ["acme/web ist nicht sichtbar"]
    );
    let jira = ws.sync.state_of(Source::Jira).unwrap();
    assert_eq!(jira.warning_lines(Lang::En), ["Review: approved"]);
    assert_eq!(jira.warning_lines(Lang::De), ["Review: genehmigt"]);

    // Saving keeps what was stored, and the saved state is stable.
    let saved = persist::save_with_revision(&ws, revision);
    for kept in [
        "\"text\":\"Status von BAU-142 auf „Review“ gesetzt\"",
        "\"agent_note\":\"Der Agent arbeitet an der Umsetzung\"",
        "\"warnings\":[\"acme/web ist nicht sichtbar\"]",
    ] {
        assert!(saved.contains(kept), "{kept} is lost");
    }
    let (again, _) = persist::load_with_revision(&saved).expect("the saved state loads");
    assert_eq!(again, ws);
    assert_eq!(persist::save_with_revision(&again, revision), saved);
}

/// New log entries, messages of a run, notes of the agent and hints of the sync carry the message
/// and no German plain text next to it.
#[test]
fn new_messages_are_stored_without_a_plain_text() {
    let mut ws = busy_workspace();
    let saved = persist::save_with_revision(&ws, 1);
    let (loaded, _) = persist::load_with_revision(&saved).expect("the saved state loads");
    assert_eq!(loaded, ws);
    for entry in &ws.log {
        assert!(entry.text.is_empty() && entry.msg.is_some(), "{entry:?}");
    }
    let run = ws.run("BAU-142").unwrap();
    // A message of the system has its code and no text; the words of the agent stay words.
    assert!(run.messages.iter().any(|m| m.msg.is_some()));
    for message in run.messages.iter().filter(|m| m.msg.is_some()) {
        assert!(message.text.is_empty(), "{message:?}");
    }
    let item = ws.item("bau142").unwrap();
    assert!(item.agent_note.is_none() && item.agent_note_msg.is_some());
    assert!(!saved.contains("\"agent_note\":\"Der Agent"));

    // The hints of a sync, as messages only.
    let hint = Msg::raw("Hinweis");
    ws.sync.sources.push(cockpit_core::SourceState {
        source: Source::Jira,
        problem: None,
        last_ok_ms: Some(1),
        last_try_ms: 1,
        since_ms: None,
        warnings: Vec::new(),
        warning_msgs: vec![hint.clone()],
    });
    let saved = persist::save_with_revision(&ws, 2);
    assert!(saved.contains("\"warning_msgs\"") && !saved.contains("\"warnings\""));
    let (loaded, _) = persist::load_with_revision(&saved).expect("the saved state loads");
    assert_eq!(
        loaded
            .sync
            .state_of(Source::Jira)
            .unwrap()
            .warning_lines(Lang::En),
        ["Hinweis"]
    );
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
/// underscores, upper case, a leading dot in the name, an Enterprise Managed User's underscore, a
/// hyphen at the end of an owner) keep loading; a name no repo can have does not.
#[test]
fn stored_pull_request_keys_with_usual_github_names_still_load() {
    for key in [
        "acme/app#3",
        "Acme/App.JS#4",
        "my-org/my_repo.name#5",
        "acme/.github#6",
        "a/b#7",
        "octocat_acme/app#8",
        "old-name-/app#9",
        "a--b/c#10",
    ] {
        let loaded: cockpit_core::PrKey =
            serde_json::from_str(&format!("\"{key}\"")).unwrap_or_else(|e| panic!("{key}: {e}"));
        assert_eq!(loaded.to_string(), key);
    }
    for key in [
        "acme/..#1",
        "../x#1",
        "acme/a/b#1",
        "a.b/c#1",
        "a b/c#1",
        "-a/b#1",
        "acme/-b#1",
    ] {
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
