//! Sprints and the relation to pull requests across syncs and across the wire.

use super::*;
use crate::model::{Sprint, SprintState, TicketStatus};
use crate::{Filter, InboxFilter, Ownership, SourceFilter};

fn in_sprint(key: &str, state: SprintState) -> Item {
    Item {
        sprint: Some(Sprint {
            name: "Sprint 7".into(),
            state,
            start: String::new(),
            end: String::new(),
        }),
        ..ticket(key, "T")
    }
}

fn shown(ws: &Workspace, by: &InboxFilter) -> Vec<String> {
    ws.inbox(Filter::All, SourceFilter::All, "", T, by)
        .iter()
        .map(|i| i.key.clone())
        .collect()
}

#[test]
fn a_ticket_that_the_sync_no_longer_delivers_leaves_the_current_sprint() {
    let (ws, _) = merge(
        &Workspace::default(),
        &jira(vec![
            in_sprint("ABC-1", SprintState::Active),
            in_sprint("ABC-2", SprintState::Active),
        ]),
        T,
    );
    let mine = InboxFilter {
        ownership: Ownership::All,
        ..Default::default()
    };
    assert_eq!(shown(&ws, &mine), ["ABC-1", "ABC-2"]);

    // The sprint ended: the query no longer delivers ABC-2, and ABC-1 now says „closed“.
    let (ws, changes) = merge(
        &ws,
        &jira(vec![in_sprint("ABC-1", SprintState::Closed)]),
        T + 1,
    );
    assert!(
        ws.item_by_key("ABC-2").is_none(),
        "gone, not stuck as current"
    );
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, Change::ItemRemoved { key, .. } if key == "ABC-2"))
    );
    assert!(shown(&ws, &mine).is_empty(), "closed is not current");
    // Still reachable while the sync delivers it.
    let open = InboxFilter {
        sprint: crate::SprintView::AllOpen,
        ..mine
    };
    assert_eq!(shown(&ws, &open), ["ABC-1"]);
}

#[test]
fn a_failed_source_keeps_its_tickets_in_their_sprint() {
    let (ws, _) = merge(
        &Workspace::default(),
        &jira(vec![in_sprint("ABC-1", SprintState::Active)]),
        T,
    );
    let failed = Remote {
        sources: vec![SourceData {
            source: Source::Jira,
            outcome: Outcome::Failed(Problem::Offline),
        }],
    };
    let (ws, _) = merge(&ws, &failed, T + 1);
    assert!(ws.item_by_key("ABC-1").is_some());
}

#[test]
fn done_tickets_are_hidden_by_default_and_shown_on_request() {
    let mut done = in_sprint("ABC-1", SprintState::Active);
    done.status = TicketStatus::Done;
    let (ws, _) = merge(&Workspace::default(), &jira(vec![done]), T);
    let by = InboxFilter {
        ownership: Ownership::All,
        ..Default::default()
    };
    assert!(shown(&ws, &by).is_empty());
    let by = InboxFilter {
        hide_done: false,
        ..by
    };
    assert_eq!(shown(&ws, &by), ["ABC-1"]);
}

#[test]
fn the_new_fields_are_optional_on_the_wire_and_absent_when_unset() {
    // Old entries (without sprint, update time and relations) still load …
    let old = r##"{"kind":"Ticket","source":"Jira","reason":"Assigned","scope":"Mine",
        "owner":{"name":"Du","initials":"DU","tone":"Blue"}}"##;
    let item: Item = serde_json::from_str(old).unwrap();
    assert_eq!(item.sprint, None);
    assert_eq!(item.updated_ms, None);
    // … and an entry that does not use them does not write them (snapshots stay byte-identical).
    let json = serde_json::to_string(&ticket("ABC-1", "T")).unwrap();
    assert!(
        !json.contains("sprint") && !json.contains("updated_ms"),
        "{json}"
    );
    let pr = PrRef {
        number: 1,
        repo: String::new(),
        draft: false,
        checks: crate::model::Checks::Passed,
        changes_requested: false,
        merged: false,
        review_requested: false,
        author: String::new(),
        mine: false,
        requested_me: false,
        assigned_me: false,
    };
    let json = serde_json::to_string(&pr).unwrap();
    for field in ["author", "mine", "requested_me", "assigned_me"] {
        assert!(!json.contains(field), "{field} in {json}");
    }
    let old_pr: PrRef = serde_json::from_str(r#"{"number":1,"checks":"Passed"}"#).unwrap();
    assert!(!old_pr.mine && old_pr.author.is_empty());
    // Set values survive.
    let with = PrRef {
        author: "lena".into(),
        mine: true,
        requested_me: true,
        assigned_me: true,
        ..pr
    };
    let back: PrRef = serde_json::from_str(&serde_json::to_string(&with).unwrap()).unwrap();
    assert_eq!(back, with);
    let sprint = in_sprint("ABC-1", SprintState::Active);
    let back: Item = serde_json::from_str(&serde_json::to_string(&sprint).unwrap()).unwrap();
    assert_eq!(back, sprint);
}
