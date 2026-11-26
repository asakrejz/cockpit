//! Inputs for follow-up jobs of the agent, read from a pull request: review threads and the output
//! of failed checks. Pure (no network): the client fetches, this module groups, limits and shortens.
//!
//! Everything here is **data of third parties** (comments, logs). It goes to the container as
//! text in the job file and never as an instruction of Cockpit; the amounts are limited so that a
//! pull request with hundreds of comments or a huge log cannot blow up the prompt or the costs.

use serde::Serialize;

use super::dto::{Annotation, CheckRun, PullReview, ReviewComment};

/// At most this many threads go to the agent (the newest are kept).
pub const MAX_THREADS: usize = 30;
/// At most this many comments per thread (the first ones; the question is at the beginning).
pub const MAX_COMMENTS: usize = 8;
/// Longest text of one comment, in characters.
pub const MAX_COMMENT_CHARS: usize = 1_500;
/// At most this many summaries of whole reviews.
pub const MAX_NOTES: usize = 8;
/// At most this many failed checks.
pub const MAX_CHECKS: usize = 8;
/// Longest log excerpt per check, in characters (the end is kept: that is where the error is).
pub const MAX_EXCERPT_CHARS: usize = 3_000;
/// All texts of one job together: whatever goes beyond is dropped (oldest threads first).
pub const MAX_TOTAL_CHARS: usize = 60_000;
/// At most this many annotations per check.
pub const MAX_ANNOTATIONS: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadComment {
    pub author: String,
    pub text: String,
}

/// A conversation on a line of the diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Thread {
    pub path: String,
    /// `None` for outdated comments (the line no longer exists in the current diff).
    pub line: Option<u32>,
    pub comments: Vec<ThreadComment>,
}

/// The text of a whole review (the part next to the line comments).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewNote {
    pub author: String,
    /// `CHANGES_REQUESTED` or `COMMENTED`
    pub state: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ReviewInput {
    pub threads: Vec<Thread>,
    pub notes: Vec<ReviewNote>,
}

impl ReviewInput {
    pub fn is_empty(&self) -> bool {
        self.threads.is_empty() && self.notes.is_empty()
    }
}

/// A red check with a shortened excerpt of what GitHub knows about the failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedCheck {
    pub name: String,
    pub excerpt: String,
}

/// Shortens at the start (keeps the beginning), counted in characters.
pub fn head(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut t: String = text.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// Shortens at the end (keeps the end), counted in characters.
pub fn tail(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let rest: String = text.chars().skip(n - max.saturating_sub(1)).collect();
    format!("…{rest}")
}

/// Groups the line comments into threads: replies hang on the first comment of their thread.
/// Order by the first comment; at most [`MAX_THREADS`] (the newest), [`MAX_COMMENTS`] each.
pub fn threads(comments: &[ReviewComment]) -> Vec<Thread> {
    // Root of a comment: follow `in_reply_to_id` up (bounded, against cycles).
    let root = |c: &ReviewComment| -> u64 {
        let mut id = c.id;
        let mut parent = c.in_reply_to_id;
        for _ in 0..50 {
            let Some(p) = parent else { break };
            id = p;
            parent = comments
                .iter()
                .find(|x| x.id == p)
                .and_then(|x| x.in_reply_to_id);
        }
        id
    };
    let mut out: Vec<(u64, Thread)> = Vec::new();
    for c in comments {
        let text = c.body.as_deref().unwrap_or_default().trim();
        if text.is_empty() {
            continue;
        }
        let comment = ThreadComment {
            author: c.user.login.clone(),
            text: head(text, MAX_COMMENT_CHARS),
        };
        let key = root(c);
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, t)) => {
                if t.comments.len() < MAX_COMMENTS {
                    t.comments.push(comment);
                }
            }
            None => out.push((
                key,
                Thread {
                    path: c.path.clone(),
                    line: c.line,
                    comments: vec![comment],
                },
            )),
        }
    }
    let mut all: Vec<Thread> = out.into_iter().map(|(_, t)| t).collect();
    if all.len() > MAX_THREADS {
        all.drain(..all.len() - MAX_THREADS);
    }
    fit(all)
}

/// Keeps the newest threads that fit into [`MAX_TOTAL_CHARS`].
fn fit(threads: Vec<Thread>) -> Vec<Thread> {
    let size = |t: &Thread| {
        t.comments
            .iter()
            .map(|c| c.text.chars().count())
            .sum::<usize>()
    };
    let mut total = 0;
    let mut keep = 0;
    for t in threads.iter().rev() {
        total += size(t);
        if total > MAX_TOTAL_CHARS && keep > 0 {
            break;
        }
        keep += 1;
    }
    threads[threads.len() - keep..].to_vec()
}

/// Review texts that ask for something: `CHANGES_REQUESTED` and `COMMENTED` with text.
/// Approvals and dismissed reviews are not input.
pub fn notes(reviews: &[PullReview]) -> Vec<ReviewNote> {
    let mut all: Vec<ReviewNote> = reviews
        .iter()
        .filter(|r| matches!(r.state.as_str(), "CHANGES_REQUESTED" | "COMMENTED"))
        .filter_map(|r| {
            let text = r.body.as_deref().unwrap_or_default().trim();
            (!text.is_empty()).then(|| ReviewNote {
                author: r.user.login.clone(),
                state: r.state.clone(),
                text: head(text, MAX_COMMENT_CHARS),
            })
        })
        .collect();
    if all.len() > MAX_NOTES {
        all.drain(..all.len() - MAX_NOTES);
    }
    all
}

/// Is this check red?
pub fn is_failed(run: &CheckRun) -> bool {
    super::aggregate_checks(std::slice::from_ref(run)) == cockpit_core::Checks::Failed
}

/// Text of the output of a check (title, summary, text) plus annotations, shortened.
pub fn excerpt(run: &CheckRun, annotations: &[Annotation]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(o) = &run.output {
        for t in [&o.title, &o.summary, &o.text].into_iter().flatten() {
            if !t.trim().is_empty() {
                parts.push(t.trim().to_string());
            }
        }
    }
    let failures: Vec<String> = annotations
        .iter()
        .filter(|a| a.annotation_level != "notice" && !a.message.trim().is_empty())
        .take(MAX_ANNOTATIONS)
        .map(|a| format!("{}:{}: {}", a.path, a.start_line, a.message.trim()))
        .collect();
    if !failures.is_empty() {
        parts.push(failures.join("\n"));
    }
    tail(&parts.join("\n\n"), MAX_EXCERPT_CHARS)
}

#[cfg(test)]
mod tests;
