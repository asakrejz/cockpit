//! Wire/persistence compatibility: the saved JSON must stay byte-identical across renames
//! of Rust identifiers. `wire_*.json` are the current snapshots (they write `"User"` where
//! the older versions wrote `"Du"`); `legacy_*.json` are the snapshots written before the English
//! renaming and must keep loading (the old names are read through `serde(alias)`).
//! Regenerate `wire_*` with `BLESS=1 cargo test -p cockpit-core --test wire_compat` only on purpose.

use cockpit_core::{Action, Actor, AgentOptions, Rule, TicketStatus, Workspace, demo, persist};

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
        (Action::AdvanceAgents, Actor::System),
        (Action::AdvanceAgents, Actor::System),
        (Action::AdvanceAgents, Actor::System),
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
    for name in ["wire_demo_workspace.json", "wire_busy_workspace.json"] {
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
        ("legacy_demo_workspace.json", "wire_demo_workspace.json"),
        ("legacy_busy_workspace.json", "wire_busy_workspace.json"),
    ] {
        let text = read(legacy);
        let (ws, revision) = persist::load_with_revision(&text).expect("old snapshot loads");
        assert!(
            resave(&ws, revision, &text) == read(current),
            "{legacy}: resaving must give the current snapshot"
        );
    }
}

#[test]
fn the_busy_legacy_snapshot_really_uses_the_old_name() {
    assert!(read("legacy_busy_workspace.json").contains("\"actor\":\"Du\""));
    assert!(!read("wire_busy_workspace.json").contains("\"actor\":\"Du\""));
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
