//! The list of repos to choose from: search and the sentence that says what was found.

/// Does the repo match the search? Every word of the query has to occur in `owner/name`
/// (case does not matter); an empty query matches everything.
pub fn repo_matches(repo: &str, query: &str) -> bool {
    let repo = repo.to_lowercase();
    query
        .split_whitespace()
        .all(|w| repo.contains(&w.to_lowercase()))
}

/// The feedback above the list: „23 Repos gefunden, 2 ausgewählt“. `shown` is the number after
/// the search; if it differs from `found`, it is named too.
pub fn repo_summary(found: usize, shown: usize, selected: usize, truncated: bool) -> String {
    let repos = |n: usize| {
        if n == 1 {
            "1 Repo".to_string()
        } else {
            format!("{n} Repos")
        }
    };
    let mut text = format!(
        "{} gefunden, {} ausgewählt.",
        repos(found),
        match selected {
            0 => "keins".to_string(),
            n => n.to_string(),
        }
    );
    if shown != found {
        text.push_str(&format!(" Die Suche zeigt {shown}."));
    }
    if truncated {
        text.push_str(" Es gibt noch mehr – trage weitere Repos von Hand ein.");
    }
    text
}
