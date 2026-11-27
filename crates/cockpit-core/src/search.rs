//! Text search over the inbox – pure functions, without state. The inbox filters live in
//! [`crate::filters`].

use crate::model::{Filter, Item, SourceFilter};

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
/// assert!(!matches_query(item, "does-not-exist"));
/// ```
pub fn matches_query(item: &Item, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    // The reason line and the owner (you, nobody) are searched in both languages.
    let reason = item.reason_line();
    let haystack = [
        item.title.as_str(),
        item.key.as_str(),
        reason.render(cockpit_i18n::Lang::De).as_str(),
        reason.render(cockpit_i18n::Lang::En).as_str(),
        item.owner.name_in(cockpit_i18n::Lang::De).as_str(),
        item.owner.name_in(cockpit_i18n::Lang::En).as_str(),
    ]
    .join("\n")
    .to_lowercase();
    query.split_whitespace().all(|word| haystack.contains(word))
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
