//! Search among people: matching and ranking, pure and local.
//!
//! Matching ignores case and accents („Böhm“ = „bohm“). A name that starts with the query
//! comes before one with a word that starts with it, and that before one that only contains it.
//! You come first, nobody appears twice. "Nobody" (unassigned) and the agent are never results.

use cockpit_i18n::Lang;

use crate::Person;

/// At most this many suggestions are shown.
pub const MAX_SUGGESTIONS: usize = 8;
/// Suggestions start with this many characters.
pub const MIN_QUERY_CHARS: usize = 2;

/// Lower case without accents, for comparing. Not a full Unicode normalisation – it covers
/// the Latin letters names are written with.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ą' => out.push('a'),
            'ç' | 'ć' | 'č' => out.push('c'),
            'ď' | 'đ' => out.push('d'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' => out.push('i'),
            'ł' => out.push('l'),
            'ñ' | 'ń' | 'ň' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => out.push('o'),
            'ř' => out.push('r'),
            'ś' | 'š' | 'ş' => out.push('s'),
            'ť' => out.push('t'),
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'ź' | 'ż' | 'ž' => out.push('z'),
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            other => out.push(other),
        }
    }
    out
}

/// How well a name matches: 0 = starts with the query, 1 = a word starts with it,
/// 2 = contains it, `None` = no match. `needle` must already be folded.
fn grade(name: &str, needle: &str) -> Option<u8> {
    let name = fold(name);
    if name.starts_with(needle) {
        return Some(0);
    }
    let word_start = name.match_indices(needle).any(|(i, _)| {
        name[..i]
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_alphanumeric())
    });
    if word_start {
        Some(1)
    } else if name.contains(needle) {
        Some(2)
    } else {
        None
    }
}

/// The people for a query: `local` (known from synced tickets) and `remote` (from Jira) merged,
/// ranked and cut to [`MAX_SUGGESTIONS`]. A query shorter than [`MIN_QUERY_CHARS`] finds
/// nobody. Persons with the same (folded) name count once; the local one wins.
pub fn search(local: &[Person], remote: &[Person], query: &str) -> Vec<Person> {
    let needle = fold(query.trim());
    if needle.chars().count() < MIN_QUERY_CHARS {
        return Vec::new();
    }
    let me = Person::me();
    let mut seen: Vec<String> = Vec::new();
    let mut hits: Vec<(u8, usize, Person)> = Vec::new();
    for (order, person) in local.iter().chain(remote).enumerate() {
        // You are found by the word for "you" in either language.
        let names = if person.me {
            vec![person.name_in(Lang::De), person.name_in(Lang::En)]
        } else {
            vec![person.name.clone()]
        };
        let key = fold(&names[0]);
        if person.is_agent() || person.nobody || names[0].is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if let Some(rank) = names.iter().filter_map(|n| grade(n, &needle)).min() {
            let rank = if *person == me { 0 } else { rank + 1 };
            hits.push((rank, order, person.clone()));
        }
    }
    // Inside a rank the order of arrival stays: local people (inbox order) before Jira's.
    hits.sort_by_key(|h| (h.0, h.1));
    hits.into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, _, p)| p)
        .collect()
}

#[cfg(test)]
mod tests;
