//! Suche und Filter der Inbox – reine Funktionen, ohne Zustand.

use crate::model::{Filter, Item, SourceFilter};

/// Passt der Eintrag zum Suchtext?
///
/// Durchsucht Titel, Schlüssel (`BAU-142`, `#87`), Grund und Besitzer, ohne auf
/// Groß-/Kleinschreibung zu achten. Der Text wird an Leerzeichen in Wörter geteilt;
/// jedes Wort muss irgendwo vorkommen. Ein leerer Suchtext passt auf alles.
///
/// ```
/// use cockpit_core::{demo, search::matches_query};
/// let ws = demo::workspace();
/// let item = ws.item_by_key("BAU-142").unwrap();
/// assert!(matches_query(item, "bau-142"));
/// assert!(!matches_query(item, "gibt-es-nicht"));
/// ```
pub fn matches_query(item: &Item, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let haystack = [
        item.title.as_str(),
        item.key.as_str(),
        item.reason_text.as_str(),
        item.owner.name.as_str(),
    ]
    .join("\n")
    .to_lowercase();
    query.split_whitespace().all(|word| haystack.contains(word))
}

/// Filtert Einträge nach Besitzer, Quelle und Suchtext; die Reihenfolge bleibt erhalten.
pub fn search<'a>(
    items: &'a [Item],
    filter: Filter,
    source: SourceFilter,
    query: &str,
) -> Vec<&'a Item> {
    items
        .iter()
        .filter(|i| filter.matches(i.scope) && source.matches(i.source))
        .filter(|i| matches_query(i, query))
        .collect()
}

#[cfg(test)]
mod tests {
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
        // Quelle und Text zusammen: ein Jira-Schlüssel findet in GitHub nichts.
        assert!(search(&ws.items, Filter::All, SourceFilter::GitHub, "BAU-142").is_empty());
    }

    #[test]
    fn workspace_search_matches_visible_without_query() {
        let ws = demo::workspace();
        for f in [Filter::All, Filter::Mine, Filter::Team] {
            assert_eq!(
                keys(&ws.search(f, SourceFilter::All, "")),
                keys(&ws.visible(f))
            );
        }
    }
}
