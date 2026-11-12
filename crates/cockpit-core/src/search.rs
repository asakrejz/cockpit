//! Search and filter of the inbox – pure functions, without state.

use crate::model::{Filter, Item, SourceFilter, TicketStatus};

/// Does the entry match the search text?
///
/// Searches title, key (`BAU-142`, `#87`), reason and owner, without regard to
/// case. The text is split into words at spaces;
/// every word must occur somewhere. An empty search text matches everything.
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

/// The inbox filters by repo, person, status and label. Each field `None` = „all“.
/// The Jira project is not part of it: it is a setting of the workspace
/// ([`crate::Workspace::in_project`]). The value is pure UI state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InboxFilter {
    /// `owner/name`; entries without a repo (tickets) drop out as soon as a repo is chosen.
    pub repo: Option<String>,
    /// Name of the owner.
    pub person: Option<String>,
    /// See [`Item::effective_status`].
    pub status: Option<TicketStatus>,
    /// Without regard to case.
    pub label: Option<String>,
}

impl InboxFilter {
    /// Is nothing restricted?
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Does the entry match all chosen values?
    ///
    /// ```
    /// use cockpit_core::{InboxFilter, demo};
    /// let ws = demo::workspace();
    /// let filter = InboxFilter { label: Some("docs".into()), ..Default::default() };
    /// assert!(filter.matches(ws.item_by_key("DOC-7").unwrap()));
    /// assert!(!filter.matches(ws.item_by_key("BAU-142").unwrap()));
    /// ```
    pub fn matches(&self, item: &Item) -> bool {
        self.repo
            .as_ref()
            .is_none_or(|r| item.repo.as_ref() == Some(r))
            && self.person.as_ref().is_none_or(|p| item.owner.name == *p)
            && self.status.is_none_or(|s| item.effective_status() == s)
            && self.label.as_ref().is_none_or(|l| {
                item.labels
                    .iter()
                    .any(|own| own.to_lowercase() == l.to_lowercase())
            })
    }

    /// Keeps the entries that match; the order is preserved.
    pub fn apply<'a>(&self, items: Vec<&'a Item>) -> Vec<&'a Item> {
        items.into_iter().filter(|i| self.matches(i)).collect()
    }
}

/// Filters entries by owner, source and search text; the order is preserved.
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

    fn keys_of(filter: &InboxFilter) -> Vec<String> {
        let ws = demo::workspace();
        keys(&filter.apply(ws.items.iter().collect()))
    }

    #[test]
    fn the_default_filter_keeps_everything() {
        let ws = demo::workspace();
        let filter = InboxFilter::default();
        assert!(filter.is_default());
        assert_eq!(
            filter.apply(ws.items.iter().collect()).len(),
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
        assert_eq!(
            keys_of(&f),
            ["BAU-142", "BAU-138"],
            "ohne Groß-/Kleinschreibung"
        );
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
            [crate::demo::DEMO_REPO_TOKENS, crate::DEMO_REPO]
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
}
