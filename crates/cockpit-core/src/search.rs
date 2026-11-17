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
mod tests;
