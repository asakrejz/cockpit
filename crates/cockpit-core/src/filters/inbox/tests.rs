use super::*;
use crate::{TicketStatus, demo};

fn keys(items: &[&Item]) -> Vec<String> {
    items.iter().map(|i| i.key.clone()).collect()
}

/// The filter without the start restrictions on sprint and done tickets (the demo tickets have no sprint).
fn unrestricted(filter: &InboxFilter) -> InboxFilter {
    InboxFilter {
        sprint: crate::SprintView::AllOpen,
        hide_done: false,
        ..filter.clone()
    }
}

fn keys_of(filter: &InboxFilter) -> Vec<String> {
    let ws = demo::workspace();
    keys(&unrestricted(filter).apply(ws.items.iter().collect(), 0))
}

#[test]
fn the_default_filter_keeps_everything() {
    let ws = demo::workspace();
    let filter = unrestricted(&InboxFilter::default());
    assert!(
        !filter.is_default(),
        "the start state restricts sprint and done"
    );
    assert_eq!(
        filter.apply(ws.items.iter().collect(), 0).len(),
        ws.items.len()
    );
}

#[test]
fn filters_by_repo() {
    let f = InboxFilter {
        repo: Some(crate::demo::DEMO_REPO_TOKENS.into()),
        ..Default::default()
    };
    assert_eq!(keys_of(&f), ["#12"]);
    let f = InboxFilter {
        repo: Some(crate::demo::DEMO_REPO.into()),
        ..Default::default()
    };
    assert_eq!(keys_of(&f), ["#87", "#85"], "tickets have no repo");
}

#[test]
fn filters_by_person_status_and_label() {
    let f = InboxFilter {
        person: Some("Jonas".into()),
        ..Default::default()
    };
    assert_eq!(keys_of(&f), ["#87"]);
    // Tickets: own status; pull requests: the one they suggest (#87: review requested).
    let f = InboxFilter {
        status: Some(TicketStatus::Review),
        ..Default::default()
    };
    assert_eq!(keys_of(&f), ["#87", "BAU-138"]);
    let f = InboxFilter {
        label: Some("A11Y".into()),
        ..Default::default()
    };
    assert_eq!(keys_of(&f), ["BAU-142", "BAU-138"], "case-insensitive");
}

#[test]
fn filters_combine_with_and() {
    let f = InboxFilter {
        label: Some("bug".into()),
        repo: Some(crate::demo::DEMO_REPO.into()),
        ..Default::default()
    };
    assert_eq!(keys_of(&f), ["#85"]);
    let f = InboxFilter {
        label: Some("docs".into()),
        person: Some("Jonas".into()),
        ..Default::default()
    };
    assert!(keys_of(&f).is_empty());
    assert!(!f.is_default());
}

#[test]
fn the_choices_come_from_the_workspace() {
    let ws = demo::workspace();
    assert_eq!(
        ws.item_repos(),
        [crate::demo::DEMO_REPO, crate::demo::DEMO_REPO_TOKENS]
    );
    assert_eq!(
        ws.labels(),
        ["a11y", "agent", "bug", "design", "docs", "feature"]
    );
    let names: Vec<_> = ws
        .owners()
        .into_iter()
        .map(|p| p.name_in(cockpit_i18n::Lang::De))
        .collect();
    assert_eq!(names, ["Du", "Jonas", "Mira"]);
}

#[test]
fn labels_ignore_case_and_empty_values() {
    let mut ws = demo::workspace();
    ws.items[0].labels = vec!["Bug".into(), String::new()];
    assert_eq!(
        ws.labels()
            .iter()
            .filter(|l| l.eq_ignore_ascii_case("bug"))
            .count(),
        1
    );
    assert!(!ws.labels().iter().any(String::is_empty));
}
