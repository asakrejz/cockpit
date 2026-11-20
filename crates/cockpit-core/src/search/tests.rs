use super::*;
use crate::{Source, demo};

fn keys(items: &[&Item]) -> Vec<String> {
    items.iter().map(|i| i.key.clone()).collect()
}

#[test]
fn empty_query_keeps_everything() {
    let ws = demo::workspace();
    for q in ["", "   "] {
        assert_eq!(
            search(&ws.items, Filter::All, SourceFilter::All, q).len(),
            ws.items.len()
        );
    }
}

#[test]
fn finds_by_key_ignoring_case() {
    let ws = demo::workspace();
    let hits = search(&ws.items, Filter::All, SourceFilter::All, "bau-142");
    assert_eq!(keys(&hits), ["BAU-142"]);
    let hits = search(&ws.items, Filter::All, SourceFilter::All, "#87");
    assert_eq!(keys(&hits), ["#87"]);
}

#[test]
fn finds_by_title_reason_and_owner() {
    let mut ws = demo::workspace();
    ws.items[0].title = "Fokus-Ring fehlt".into();
    ws.items[0].reason_text = "Neuer Kommentar von Lena".into();
    ws.items[0].owner.name = "Jonas Ärmel".into();
    let first = ws.items[0].key.clone();
    for q in ["FOKUS", "kommentar von lena", "jonas", "ÄRMEL"] {
        let hits = search(&ws.items, Filter::All, SourceFilter::All, q);
        assert!(keys(&hits).contains(&first), "{q}");
    }
}

#[test]
fn every_word_must_match() {
    let mut ws = demo::workspace();
    ws.items[0].title = "Fokus-Ring fehlt".into();
    let hits = search(&ws.items, Filter::All, SourceFilter::All, "ring fehlt");
    assert!(!hits.is_empty());
    let hits = search(&ws.items, Filter::All, SourceFilter::All, "ring qwertz");
    assert!(hits.is_empty());
}

#[test]
fn source_filter_splits_jira_and_github() {
    let ws = demo::workspace();
    let jira = search(&ws.items, Filter::All, SourceFilter::Jira, "");
    let github = search(&ws.items, Filter::All, SourceFilter::GitHub, "");
    assert!(!jira.is_empty() && !github.is_empty());
    assert!(jira.iter().all(|i| i.source == Source::Jira));
    assert!(github.iter().all(|i| i.source == Source::GitHub));
    assert_eq!(jira.len() + github.len(), ws.items.len());
}

#[test]
fn filters_combine_and_keep_order() {
    let ws = demo::workspace();
    let all = search(&ws.items, Filter::Mine, SourceFilter::All, "");
    let github = search(&ws.items, Filter::Mine, SourceFilter::GitHub, "");
    let expected: Vec<_> = all
        .iter()
        .filter(|i| i.source == Source::GitHub)
        .map(|i| i.key.clone())
        .collect();
    assert_eq!(keys(&github), expected);
    // Source and text together: a Jira key finds nothing in GitHub.
    assert!(search(&ws.items, Filter::All, SourceFilter::GitHub, "BAU-142").is_empty());
}

#[test]
fn workspace_search_is_the_rule_based_subset_of_visible() {
    let ws = demo::workspace();
    let now = crate::attention::day_start_ms("2026-11-11").unwrap();
    for f in [Filter::All, Filter::Mine, Filter::Team] {
        let found = keys(&ws.search(f, SourceFilter::All, "", now));
        let visible = keys(&ws.visible(f));
        assert!(found.iter().all(|k| visible.contains(k)), "{f:?}");
        assert!(found.len() < visible.len() || f == Filter::Mine, "{f:?}");
        let expected: Vec<_> = visible.into_iter().filter(|k| found.contains(k)).collect();
        assert_eq!(found, expected, "Reihenfolge bleibt");
    }
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
        repo: Some(crate::DEMO_REPO.into()),
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
        repo: Some(crate::DEMO_REPO.into()),
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
        [crate::DEMO_REPO, crate::demo::DEMO_REPO_TOKENS]
    );
    assert_eq!(
        ws.labels(),
        ["a11y", "agent", "bug", "design", "docs", "feature"]
    );
    let names: Vec<_> = ws.owners().into_iter().map(|p| p.name).collect();
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
