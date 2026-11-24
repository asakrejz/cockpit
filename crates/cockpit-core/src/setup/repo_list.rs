//! The list of repos to choose from: search, the limit of what is drawn and the sentence that
//! says what was found.

use cockpit_i18n::{ArgValue, Msg, msg};

/// At most this many repos are drawn; the search narrows the rest down.
pub const REPOS_SHOWN: usize = 100;

/// Does the repo match the search? Every word of the query has to occur in `owner/name`
/// (case does not matter); an empty query matches everything.
pub fn repo_matches(repo: &str, query: &str) -> bool {
    let repo = repo.to_lowercase();
    query
        .split_whitespace()
        .all(|w| repo.contains(&w.to_lowercase()))
}

/// What a repo list draws for a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoPage<T> {
    /// The matches in their order, at most [`REPOS_SHOWN`].
    pub shown: Vec<T>,
    /// How many entries match in all, drawn or not.
    pub matching: usize,
}

impl<T> RepoPage<T> {
    /// More entries match than are drawn: the list says to narrow the search down.
    pub fn too_many(&self) -> bool {
        self.matching > self.shown.len()
    }
}

/// The part of `items` that matches `query` (see [`repo_matches`]), in the given order, cut at
/// [`REPOS_SHOWN`]. `name` gives the `owner/name` of an entry.
pub fn repo_page<T: Clone>(items: &[T], name: impl Fn(&T) -> &str, query: &str) -> RepoPage<T> {
    let mut shown = Vec::new();
    let mut matching = 0;
    for item in items.iter().filter(|i| repo_matches(name(i), query)) {
        if shown.len() < REPOS_SHOWN {
            shown.push(item.clone());
        }
        matching += 1;
    }
    RepoPage { shown, matching }
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
