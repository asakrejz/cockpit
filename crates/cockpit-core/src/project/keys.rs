//! Ticket keys of GitHub issues (`acme/app#123`) and how they appear in texts.

/// The key of an issue: `owner/name#123`.
///
/// ```
/// use cockpit_core::project::issue_key;
/// assert_eq!(issue_key("acme/app", 123), "acme/app#123");
/// ```
pub fn issue_key(repo: &str, number: u32) -> String {
    format!("{repo}#{number}")
}

/// The key of a draft issue (it has no number): `acme/3#draft-1a2b3c`, from the project and the
/// last characters of the item's node ID.
pub fn draft_key(project: &str, item_id: &str) -> String {
    let tail: String = item_id
        .chars()
        .rev()
        .filter(char::is_ascii_alphanumeric)
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{project}#draft-{}", tail.to_lowercase())
}

/// Splits a ticket key `owner/name#123` into repo and number. `#123` alone has no repo and is
/// no ticket key (it is the short form, see [`short_key`]).
///
/// ```
/// use cockpit_core::project::parse_issue_key;
/// assert_eq!(parse_issue_key("acme/app#7"), Some(("acme/app".to_string(), 7)));
/// assert_eq!(parse_issue_key("#7"), None);
/// assert_eq!(parse_issue_key("BAU-142"), None);
/// ```
pub fn parse_issue_key(key: &str) -> Option<(String, u32)> {
    let (repo, number) = key.trim().rsplit_once('#')?;
    if !repo_valid(repo) {
        return None;
    }
    Some((repo.to_string(), number.parse().ok()?))
}

/// The short form inside the issue's own repo: `acme/app#123` → `#123`.
pub fn short_key(key: &str) -> String {
    match key.rsplit_once('#') {
        Some((_, n)) if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => format!("#{n}"),
        _ => key.to_string(),
    }
}

/// A repo name `owner/name` (exactly one slash).
pub fn repo_valid(repo: &str) -> bool {
    crate::model::repo_name_valid(repo) && repo.matches('/').count() == 1
}

/// A reference to an issue found in a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueRef {
    /// `owner/name`; for a short `#123` the repo of the text (the default given to [`issue_refs`]).
    pub repo: String,
    pub number: u32,
    /// Written as `Fixes #123`, `closes …`, `resolved …`: merging the pull request closes it.
    pub closing: bool,
}

impl IssueRef {
    pub fn key(&self) -> String {
        issue_key(&self.repo, self.number)
    }
}

const CLOSING: &[&str] = &[
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];

/// All issue references in a text, in order of appearance, each once: `#123` (in the repo
/// `default_repo`), `owner/repo#123` and `https://github.com/owner/repo/issues/123`. A reference
/// preceded by a closing keyword (`Fixes #123`, `closes: acme/app#4`) is marked as closing.
/// Jira-style keys (`BAU-142`) and things like `abc#12` are not references.
///
/// ```
/// use cockpit_core::project::issue_refs;
/// let refs = issue_refs("Fixes #12, see acme/web#3 and #4.", "acme/app");
/// assert_eq!(refs.len(), 3);
/// assert_eq!((refs[0].key(), refs[0].closing), ("acme/app#12".to_string(), true));
/// assert_eq!((refs[1].key(), refs[1].closing), ("acme/web#3".to_string(), false));
/// assert_eq!(refs[2].key(), "acme/app#4");
/// ```
pub fn issue_refs(text: &str, default_repo: &str) -> Vec<IssueRef> {
    let mut out: Vec<IssueRef> = Vec::new();
    let mut add = |repo: String, number: u32, closing: bool| match out
        .iter_mut()
        .find(|r| r.repo == repo && r.number == number)
    {
        Some(r) => r.closing |= closing,
        None => out.push(IssueRef {
            repo,
            number,
            closing,
        }),
    };
    let bytes = text.as_bytes();
    let repo_char = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/');
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            // Digits after the hash.
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            let after_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
            if end > start && after_ok && end - start <= 9 {
                // The token in front of the hash.
                let mut from = i;
                while from > 0 && repo_char(bytes[from - 1]) {
                    from -= 1;
                }
                let before = &text[from..i];
                // `from` sits on a char boundary because every repo char is ASCII.
                let repo = if before.is_empty() {
                    Some(default_repo.to_string()).filter(|r| !r.is_empty())
                } else if repo_valid(before) {
                    Some(before.to_string())
                } else {
                    None
                };
                if let (Some(repo), Ok(n)) = (repo, text[start..end].parse::<u32>()) {
                    add(repo, n, closing_before(&text[..from]));
                }
            }
            i = end.max(i + 1);
            continue;
        }
        if bytes[i] == b'/' && text[i..].starts_with("/issues/") {
            // A web address: `…/owner/repo/issues/123`.
            let start = i + "/issues/".len();
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            let mut from = i;
            while from > 0 && repo_char(bytes[from - 1]) {
                from -= 1;
            }
            let path = &text[from..i];
            let repo = path.rsplit('/').take(2).collect::<Vec<_>>();
            if end > start && end - start <= 9 && repo.len() == 2 {
                let candidate = format!("{}/{}", repo[1], repo[0]);
                if repo_valid(&candidate)
                    && let Ok(n) = text[start..end].parse::<u32>()
                {
                    // Only github-looking addresses: the host part is whatever sits before.
                    add(candidate, n, false);
                }
            }
            i = end.max(i + 1);
            continue;
        }
        i += 1;
    }
    out
}

/// Does the text in front of a reference end with a closing keyword (`Fixes `, `closes: `)?
fn closing_before(before: &str) -> bool {
    let trimmed = before.trim_end_matches([' ', '\t', ':']);
    if trimmed.len() == before.len() {
        // A keyword needs a separator: `fixes#1` is no closing reference.
        return false;
    }
    let word: String = trimmed
        .chars()
        .rev()
        .take_while(char::is_ascii_alphabetic)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    CLOSING.contains(&word.to_lowercase().as_str())
}
