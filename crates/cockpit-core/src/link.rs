//! Link ticket and pull request: which tickets a pull request names.
//!
//! One rule for the whole workspace: [`PrTexts::ticket_keys`] reads the branch, the title and the
//! description of a pull request and returns every ticket it names.
//!
//! A Jira key consists of letters, a hyphen and digits (`BAU-142`). It only counts when it
//! stands as a whole word: `BAU-1420` does not contain `BAU-142`, and `XBAU-142` does not
//! either. Upper and lower case do not matter (`bau-142-toggle`), separators like `/`, `_`, `:`,
//! `.` or spaces delimit the word. In a web address (`https://…`) only a whole path segment or
//! query value counts (`…/browse/BAU-142`, `?selectedIssue=BAU-142`), not a piece of a longer
//! slug (`/blog/bau-142-story`). A GitHub issue is named as `#12` (in the pull request's own
//! repo), `acme/app#12`, `Fixes #12` or by the address of the issue on github.com, see
//! [`crate::project::issue_refs`].

use std::borrow::Cow;

use crate::model::Item;
use crate::project::issue_refs;
use crate::review::Review;

/// The texts of a pull request in which a ticket key can stand.
#[derive(Clone, Copy, Debug)]
pub struct PrTexts<'a> {
    /// `owner/name` of the pull request: the repo a short `#12` points to. Empty = unknown, then
    /// a short reference names nothing.
    pub repo: &'a str,
    pub branch: &'a str,
    pub title: &'a str,
    pub description: &'a str,
}

impl<'a> PrTexts<'a> {
    /// The texts of a review.
    pub fn of_review(review: &'a Review) -> Self {
        Self {
            repo: &review.repo,
            branch: &review.branch,
            title: &review.title,
            description: &review.description,
        }
    }

    /// The texts of a pull request entry (the list item).
    pub fn of_item(item: &'a Item) -> Self {
        Self {
            repo: item.pr.as_ref().map_or("", |p| p.repo.as_str()),
            branch: item.branch.as_deref().unwrap_or_default(),
            title: &item.title,
            description: &item.description,
        }
    }

    /// The keys of every ticket these texts name, each once: first the Jira keys (uppercased,
    /// `BAU-142`) in the order branch, title, description, then the GitHub issues
    /// (`acme/app#12`). Compare with a ticket's key ignoring case.
    ///
    /// ```
    /// use cockpit_core::link::PrTexts;
    /// let pr = PrTexts {
    ///     repo: "acme/app",
    ///     branch: "feature/bau-142-fokus",
    ///     title: "Fokus",
    ///     description: "Fixes #12 and part of APP-7.",
    /// };
    /// assert_eq!(pr.ticket_keys(), ["BAU-142", "APP-7", "acme/app#12"]);
    /// ```
    pub fn ticket_keys(&self) -> Vec<String> {
        let texts = [self.branch, self.title, self.description];
        let mut keys: Vec<String> = Vec::new();
        let mut add = |key: String| {
            if !keys.contains(&key) {
                keys.push(key);
            }
        };
        for text in texts {
            ticket_keys(text).into_iter().for_each(&mut add);
        }
        for text in texts {
            issue_refs(text, self.repo)
                .iter()
                .for_each(|r| add(r.key()));
        }
        keys
    }
}

/// All Jira keys in a text, uppercased and in order of appearance.
///
/// ```
/// use cockpit_core::link::ticket_keys;
/// assert_eq!(ticket_keys("feature/bau-142-fokus"), ["BAU-142"]);
/// assert_eq!(ticket_keys("https://acme.atlassian.net/browse/BAU-142"), ["BAU-142"]);
/// assert!(ticket_keys("https://example.com/blog/bau-142-story").is_empty());
/// ```
pub fn ticket_keys(text: &str) -> Vec<String> {
    let text = without_web_address_slugs(text);
    let text = text.as_ref();
    // The text breaks into words of letters and digits and the separators between them.
    let mut words: Vec<(&str, &str)> = Vec::new(); // (word, separator after it)
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                let end = text[i..]
                    .find(char::is_alphanumeric)
                    .map_or(text.len(), |n| i + n);
                words.push((&text[s..i], &text[i..end]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        words.push((&text[s..], ""));
    }

    let letters = |w: &str| !w.is_empty() && w.chars().all(char::is_alphabetic);
    let digits = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit());
    let mut keys = Vec::new();
    for pair in words.windows(2) {
        let ((project, sep), (number, _)) = (pair[0], pair[1]);
        if sep == "-" && letters(project) && project.chars().count() >= 2 && digits(number) {
            let key = format!("{project}-{number}").to_uppercase();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

/// The text with its web addresses (`http://…`, `https://…`) reduced to the words that can be a
/// ticket: a whole path segment or query value that is a key (`…/browse/BAU-142`,
/// `?selectedIssue=BAU-142`). The rest of an address (`/blog/bau-142-story`, `?ref=abc-1-x`) is
/// dropped, so a piece of a slug is not read as a ticket.
fn without_web_address_slugs(text: &str) -> Cow<'_, str> {
    if !text.contains("://") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        // The scheme is the ASCII letters in front of `://`.
        let scheme_start = at
            - rest[..at]
                .chars()
                .rev()
                .take_while(char::is_ascii_alphabetic)
                .count();
        let scheme = &rest[scheme_start..at];
        let url_start = at + "://".len();
        let url_end = rest[url_start..]
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '`' | ')' | ']')
            })
            .map_or(rest.len(), |n| url_start + n);
        out.push_str(&rest[..scheme_start]);
        if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
            // A full stop or comma at the end belongs to the sentence, not to the address.
            let url = rest[url_start..url_end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
            for segment in url.split(['/', '?', '#', '&', '=']) {
                if ticket_keys(segment) == [segment.to_uppercase()] {
                    out.push(' ');
                    out.push_str(segment);
                }
            }
            out.push(' ');
        } else {
            out.push_str(&rest[scheme_start..url_end]);
        }
        rest = &rest[url_end..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests;
