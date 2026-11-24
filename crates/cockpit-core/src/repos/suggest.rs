//! Which repos to offer when the agent starts on a ticket – pure ranking, no I/O.
//!
//! The picker lists every repo the credential can see; this module only decides which of them
//! come first and why. The order of the reasons is fixed:
//!
//! 0. the repo of a GitHub issue ticket (the issue lives there, so the work does too),
//! 1. repos earlier runs used for tickets of the same Jira project (remembered from the runs in
//!    the database, no configuration),
//! 2. repos of pull requests or branches already linked to the ticket,
//! 3. repos the ticket text mentions (`owner/name` or a GitHub address),
//! 4. the default repo of the configuration (`[agent] repo`).
//!
//! Every suggestion is one repo with one reason. The start dialog selects one repo today; the
//! same ranking serves a later choice of several repos.

use serde::{Deserialize, Serialize};

use super::AgentRepos;
use crate::model::repo_name_valid;

/// Why a repo is suggested. The interface turns it into a short text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuggestReason {
    /// The ticket is an issue of this repo (GitHub Projects).
    Issue,
    /// A run for a ticket of the same Jira project worked in it (`runs` times).
    PreviousRun { project: String, runs: usize },
    /// A pull request or branch of the ticket lives in it.
    LinkedPr,
    /// The ticket text names it.
    Mentioned,
    /// It is the default repo of the configuration.
    Default,
}

/// One suggested repo (`owner/name`) with the reason.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RepoSuggestion {
    pub repo: String,
    pub reason: SuggestReason,
}

/// Everything the ranking looks at.
#[derive(Clone, Copy, Debug, Default)]
pub struct SuggestInput<'a> {
    /// The repo of a GitHub issue ticket (`acme/app` for `acme/app#12`).
    pub issue_repo: Option<&'a str>,
    /// Jira project of the ticket (`WEB`).
    pub project: Option<&'a str>,
    /// Earlier runs as `(Jira project, repo)`, oldest first.
    pub used: &'a [(String, String)],
    /// Repos of pull requests or branches that belong to the ticket.
    pub linked: &'a [String],
    /// Title, description and comments of the ticket.
    pub text: &'a str,
    /// The configured repos; their default is suggested last.
    pub repos: Option<&'a AgentRepos>,
    /// The repos the credential can see, if known. A bare `owner/name` in the text only counts
    /// when it is in this list (otherwise `docs/readme` would be a "repo"); a GitHub address
    /// always counts.
    pub visible: Option<&'a [String]>,
}

/// The suggestions in order, every repo once (its best reason wins).
///
/// ```
/// use cockpit_core::repos::suggest::{SuggestInput, SuggestReason, suggest_repos};
/// let used = vec![("WEB".to_string(), "acme/web".to_string())];
/// let s = suggest_repos(&SuggestInput {
///     project: Some("web"),
///     used: &used,
///     repos: None,
///     ..Default::default()
/// });
/// assert_eq!(s[0].repo, "acme/web");
/// assert_eq!(s[0].reason, SuggestReason::PreviousRun { project: "WEB".into(), runs: 1 });
/// ```
pub fn suggest_repos(input: &SuggestInput) -> Vec<RepoSuggestion> {
    let mut out: Vec<RepoSuggestion> = Vec::new();
    let mut add = |repo: &str, reason: SuggestReason| {
        if repo_name_valid(repo) && !out.iter().any(|s| s.repo.eq_ignore_ascii_case(repo)) {
            out.push(RepoSuggestion {
                repo: repo.to_string(),
                reason,
            });
        }
    };

    // 0. A GitHub issue belongs to its own repo.
    if let Some(repo) = input.issue_repo {
        add(repo, SuggestReason::Issue);
    }

    // 1. Earlier runs for the same project: most used first, ties by the latest use.
    if let Some(project) = input.project {
        let mut counted: Vec<(&str, usize, usize)> = Vec::new(); // (repo, runs, last index)
        for (i, (p, repo)) in input.used.iter().enumerate() {
            if !p.eq_ignore_ascii_case(project) || repo.is_empty() {
                continue;
            }
            match counted
                .iter_mut()
                .find(|(r, ..)| r.eq_ignore_ascii_case(repo))
            {
                Some(c) => {
                    c.1 += 1;
                    c.2 = i;
                }
                None => counted.push((repo, 1, i)),
            }
        }
        counted.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
        for (repo, runs, _) in counted {
            add(
                repo,
                SuggestReason::PreviousRun {
                    project: project.to_uppercase(),
                    runs,
                },
            );
        }
    }

    // 2. Pull requests and branches of the ticket.
    for repo in input.linked {
        add(repo, SuggestReason::LinkedPr);
    }

    // 3. Mentioned in the text.
    for m in repos_in_text(input.text) {
        let known = input
            .visible
            .is_some_and(|v| v.iter().any(|r| r.eq_ignore_ascii_case(&m.repo)));
        if m.from_address || known {
            add(&m.repo, SuggestReason::Mentioned);
        }
    }

    // 4. The default repo.
    if let Some(d) = input.repos.and_then(|r| r.default.as_deref()) {
        add(d, SuggestReason::Default);
    }
    out
}

/// One line of the repo picker in the start dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerRow {
    pub repo: String,
    /// Why it is suggested; `None` for the plain list of all repos.
    pub reason: Option<SuggestReason>,
}

/// The lines of the picker in the order they are shown: the suggestions first (in their order),
/// then every other repo of `all` alphabetically. `all` may be empty (GitHub could not be asked),
/// then only the suggestions show. The search and the limit of what is drawn are those of every
/// repo list (`setup::repo_page`).
///
/// ```
/// use cockpit_core::repos::suggest::{picker_rows, RepoSuggestion, SuggestReason};
/// let s = vec![RepoSuggestion { repo: "acme/web".into(), reason: SuggestReason::LinkedPr }];
/// let all = vec!["acme/api".to_string(), "acme/web".to_string()];
/// let rows = picker_rows(&s, &all);
/// let names: Vec<_> = rows.iter().map(|r| r.repo.as_str()).collect();
/// assert_eq!(names, ["acme/web", "acme/api"]);
/// assert_eq!(rows[0].reason, Some(SuggestReason::LinkedPr));
/// ```
pub fn picker_rows(suggestions: &[RepoSuggestion], all: &[String]) -> Vec<PickerRow> {
    let mut rows: Vec<PickerRow> = suggestions
        .iter()
        .map(|s| PickerRow {
            repo: s.repo.clone(),
            reason: Some(s.reason.clone()),
        })
        .collect();
    let mut rest: Vec<&String> = all
        .iter()
        .filter(|r| !rows.iter().any(|row| row.repo.eq_ignore_ascii_case(r)))
        .collect();
    rest.sort_by_key(|r| r.to_lowercase());
    rows.extend(rest.into_iter().map(|r| PickerRow {
        repo: r.clone(),
        reason: None,
    }));
    rows
}

/// A repo found in a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    pub repo: String,
    /// It came from a GitHub address (`https://github.com/acme/app/pull/3`), not from a bare
    /// `owner/name`.
    pub from_address: bool,
}

/// `owner/name` strings and GitHub addresses in a text, in order of appearance, every repo once.
///
/// ```
/// use cockpit_core::repos::suggest::repos_in_text;
/// let found = repos_in_text("See acme/app and https://github.com/acme/web/pull/7, not a/b/c.");
/// let names: Vec<_> = found.iter().map(|m| m.repo.as_str()).collect();
/// assert_eq!(names, ["acme/app", "acme/web"]);
/// ```
pub fn repos_in_text(text: &str) -> Vec<Mention> {
    let mut out: Vec<Mention> = Vec::new();
    let mut push = |repo: String, from_address: bool| {
        if let Some(m) = out.iter_mut().find(|m| m.repo.eq_ignore_ascii_case(&repo)) {
            m.from_address |= from_address;
        } else {
            out.push(Mention { repo, from_address });
        }
    };
    let separators = |c: char| {
        c.is_whitespace()
            || matches!(
                c,
                ',' | ';' | '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | '`' | '|' | '{' | '}'
            )
    };
    for raw in text.split(separators) {
        let token = raw.trim_matches(|c: char| matches!(c, '.' | ':' | '!' | '?' | '*'));
        if token.is_empty() {
            continue;
        }
        if let Some((_, rest)) = token.split_once("://") {
            let mut parts = rest.split(['/', '?', '#']);
            let host = parts.next().unwrap_or_default();
            if !(host.eq_ignore_ascii_case("github.com")
                || host.eq_ignore_ascii_case("www.github.com"))
            {
                continue;
            }
            if let (Some(owner), Some(name)) = (parts.next(), parts.next())
                && let Some(repo) = clean(owner, name)
            {
                push(repo, true);
            }
        } else if let Some((owner, name)) = token.split_once('/')
            && !name.contains('/')
            && let Some(repo) = clean(owner, name)
        {
            push(repo, false);
        }
    }
    out
}

/// `owner/name` if both parts look like a GitHub owner and repo name.
fn clean(owner: &str, name: &str) -> Option<String> {
    let name = name.strip_suffix(".git").unwrap_or(name);
    let owner_ok = (1..=39).contains(&owner.len())
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !owner.starts_with('-')
        && !owner.ends_with('-');
    let name_ok = !name.is_empty()
        && name.len() <= 100
        && name.chars().any(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    (owner_ok && name_ok).then(|| format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests;
