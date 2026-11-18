//! The list of repos to choose from: search and the sentence that says what was found.

use cockpit_i18n::{ArgValue, Msg, msg};

/// Does the repo match the search? Every word of the query has to occur in `owner/name`
/// (case does not matter); an empty query matches everything.
pub fn repo_matches(repo: &str, query: &str) -> bool {
    let repo = repo.to_lowercase();
    query
        .split_whitespace()
        .all(|w| repo.contains(&w.to_lowercase()))
}

/// The feedback above the list: "23 repos found, 2 selected". `shown` is the number after
/// the search; if it differs from `found`, it is named too.
pub fn repo_summary(found: usize, shown: usize, selected: usize, truncated: bool) -> Msg {
    let mut parts = vec![msg!(
        CoreSetupReposFound,
        found = found,
        selected = selected
    )];
    if shown != found {
        parts.push(msg!(CoreSetupReposShown, shown = shown));
    }
    if truncated {
        parts.push(msg!(CoreSetupReposMore));
    }
    msg!(CoreSetupReposSummary, parts = ArgValue::joined(parts, " "))
}
