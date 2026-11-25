//! The review as it goes to GitHub: verdict, text and position comments. Pure.

use cockpit_core::{Decision, LineKind, Review, Verdict};
use serde_json::{Value, json};

/// A review as it goes to GitHub.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewData {
    /// `COMMENT`, `APPROVE` or `REQUEST_CHANGES`
    pub verdict: &'static str,
    pub text: String,
    pub comments: Vec<LineComment>,
    /// The commit we have reviewed.
    pub sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineComment {
    pub path: String,
    pub line: u32,
    /// First line of a range of lines (on the same side). `None` = a single line.
    pub start_line: Option<u32>,
    /// `RIGHT` (added and unchanged lines) or `LEFT` (removed).
    pub page: &'static str,
    pub text: String,
}

fn verdict(v: Verdict) -> &'static str {
    match v {
        Verdict::Comment => "COMMENT",
        Verdict::Approve => "APPROVE",
        Verdict::RequestChanges => "REQUEST_CHANGES",
    }
}

/// Builds the review from the state: verdict, own text, approved suggestions of the agent
/// and all own line comments (with `suggestion` blocks). Whatever cannot be assigned to a line
/// ends up in the text of the review, so that nothing gets lost.
pub fn review_data(review: &Review, comment: &str, sha: &str) -> ReviewData {
    let mut text = comment.trim().to_string();
    let mut lines: Vec<LineComment> = Vec::new();
    let mut lose: Vec<String> = Vec::new();

    for s in review
        .suggestions
        .iter()
        .filter(|s| s.decision == Some(Decision::Approved))
    {
        // A draft without a position (path empty) cannot be placed on a line.
        let place = Some((s.path.as_str(), s.line, s.start_line, s.side))
            .filter(|(file, ..)| !file.is_empty())
            .filter(|(file, nr, _, side)| review.has_line(file, *side, *nr));
        match place {
            Some((file, nr, start, side)) => lines.push(LineComment {
                path: file.into(),
                line: nr,
                // A range only goes out if its first line exists on the same side, too.
                start_line: start.filter(|&st| st < nr && review.has_line(file, side, st)),
                page: side.id(),
                text: s.body(),
            }),
            None => lose.push(format!("{}: {}", s.location, s.body())),
        }
    }
    for c in &review.line_comments {
        let line = review
            .files
            .iter()
            .find(|f| f.name == c.file)
            .and_then(|f| f.lines.get(c.line));
        match line {
            Some(l) => lines.push(LineComment {
                path: c.file.clone(),
                line: l.no,
                start_line: None,
                page: if l.kind == LineKind::Del {
                    "LEFT"
                } else {
                    "RIGHT"
                },
                text: c.body(),
            }),
            None => lose.push(format!("{}: {}", c.file, c.body())),
        }
    }
    if !lose.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&lose.join("\n\n"));
    }
    // GitHub requires text when there are no line comments (and for „Changes“).
    if text.is_empty()
        && (review.verdict == Verdict::RequestChanges
            || (review.verdict == Verdict::Comment && lines.is_empty()))
    {
        text = match review.verdict {
            // Content for GitHub, not interface text: it is sent in English, whatever language
            // the interface speaks.
            Verdict::RequestChanges => "Changes requested.".into(),
            _ => "Review from Cockpit.".into(),
        };
    }
    ReviewData {
        verdict: verdict(review.verdict),
        text,
        comments: lines,
        sha: sha.into(),
    }
}

// ---------- Calls ----------

impl ReviewData {
    pub(super) fn body(&self) -> Value {
        let mut b = json!({ "event": self.verdict });
        if !self.text.is_empty() {
            b["body"] = json!(self.text);
        }
        if !self.sha.is_empty() {
            b["commit_id"] = json!(self.sha);
        }
        if !self.comments.is_empty() {
            b["comments"] = self
                .comments
                .iter()
                .map(|k| {
                    let mut c =
                        json!({"path": k.path, "line": k.line, "side": k.page, "body": k.text});
                    if let Some(start) = k.start_line {
                        c["start_line"] = json!(start);
                        c["start_side"] = json!(k.page);
                    }
                    c
                })
                .collect();
        }
        b
    }
}
