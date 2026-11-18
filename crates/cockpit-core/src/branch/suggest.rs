//! A branch pattern proposed from the contribution guidelines of a repo (`CONTRIBUTING.md`).
//!
//! Only a hint: the user sees the proposal and confirms it, nothing is taken over automatically.

use cockpit_i18n::{Msg, msg};
use serde::{Deserialize, Serialize};

/// Where the backend looks for the guidelines, in this order.
pub const GUIDELINE_FILES: [&str; 3] = [
    "CONTRIBUTING.md",
    ".github/CONTRIBUTING.md",
    "docs/CONTRIBUTING.md",
];

/// The branch words the guidelines may name.
const WORDS: [&str; 7] = [
    "feature", "feat", "bugfix", "fix", "hotfix", "chore", "release",
];

/// A pattern the guidelines suggest.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub pattern: String,
    /// Plain German: what was found and what to check.
    pub reason: String,
    /// The same as code and parameters; the UI renders it in its language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_msg: Option<Msg>,
}

/// Words followed by `/` that the text names as a branch prefix, in order, without doubles.
fn prefixes(text: &str) -> Vec<&'static str> {
    let lower = text.to_lowercase();
    let mut found: Vec<&'static str> = Vec::new();
    for word in WORDS {
        let needle = format!("{word}/");
        let mut from = 0;
        while let Some(at) = lower[from..].find(&needle) {
            let start = from + at;
            let before = lower[..start].chars().next_back();
            // `bugfix/` contains `fix/`: the character before must not be a letter.
            if before.is_none_or(|c| !c.is_ascii_alphabetic()) && !found.contains(&word) {
                found.push(word);
            }
            from = start + needle.len();
        }
    }
    found
}

/// Does the text show a Jira-like key (`PROJ-123`)? Letters, then `-`, then digits.
fn has_ticket_key(text: &str) -> bool {
    let b = text.as_bytes();
    (0..b.len()).any(|i| {
        if !b[i].is_ascii_uppercase() || (i > 0 && b[i - 1].is_ascii_alphanumeric()) {
            return false;
        }
        let mut j = i + 1;
        while j < b.len() && (b[j].is_ascii_uppercase() || b[j].is_ascii_digit()) {
            j += 1;
        }
        j > i + 1 && j + 1 < b.len() && b[j] == b'-' && b[j + 1].is_ascii_digit()
    })
}

/// Proposes a pattern from the text of a guideline file, or `None` if it names none of the
/// usual shapes (`feature/`, `bugfix/`, `PROJ-123`).
///
/// ```
/// use cockpit_core::branch::suggest;
/// let s = suggest("Name branches feature/PROJ-123-short-title.").unwrap();
/// assert_eq!(s.pattern, "feature/{ticket}-{slug}");
/// assert!(suggest("Please be kind.").is_none());
/// ```
pub fn suggest(text: &str) -> Option<Suggestion> {
    let mentions_branch = text.to_lowercase().contains("branch");
    let words = prefixes(text);
    let key = has_ticket_key(text);
    let has = |w: &str| words.contains(&w);
    let type_words = ["feature", "feat", "bugfix", "fix", "hotfix", "chore"];
    let types: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| type_words.contains(w))
        .collect();

    let (pattern, reason) = if has("feature") && has("fix") {
        (
            "{type}/{ticket}-{slug}".to_string(),
            msg!(CoreBranchSuggestBoth),
        )
    } else if let Some(first) = types.first() {
        let mut reason = msg!(CoreBranchSuggestFirst, first = format!("{first}/"));
        if types.len() > 1 {
            reason = msg!(
                CoreBranchSuggestMore,
                reason = reason,
                others = types[1..]
                    .iter()
                    .map(|w| format!("{w}/"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        (format!("{first}/{{ticket}}-{{slug}}"), reason)
    } else if key && mentions_branch {
        ("{ticket}-{slug}".to_string(), msg!(CoreBranchSuggestTicket))
    } else {
        return None;
    };
    Some(Suggestion {
        pattern,
        reason: reason.de(),
        reason_msg: Some(reason),
    })
}

#[cfg(test)]
mod tests;
