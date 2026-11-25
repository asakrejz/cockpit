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
    ws.items[0].owner = crate::Person::new("Jonas Ärmel", "JÄ");
    let first = ws.items[0].key.clone();
    for q in ["FOKUS", "kommentar von lena", "jonas", "ÄRMEL"] {
        let hits = search(&ws.items, Filter::All, SourceFilter::All, q);
        assert!(keys(&hits).contains(&first), "{q}");
    }
}

#[test]
fn a_reason_message_and_the_role_of_the_owner_are_found_in_both_languages() {
    let mut ws = demo::workspace();
    let mine = ws.items[0].key.clone();
    assert!(ws.items[0].owner.me);
    ws.items[0].reason_text.clear();
    ws.items[0].reason_msg = Some(cockpit_i18n::msg!(ReasonInProject));
    for q in ["im projekt", "in the project", "du", "you"] {
        let hits = search(&ws.items, Filter::All, SourceFilter::All, q);
        assert!(keys(&hits).contains(&mine), "{q}");
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
