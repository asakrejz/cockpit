//! Pull request review: diff, suggestions of the agent, approval and verdict.
//!
//! The core rule: **Nothing is sent before every suggestion has been decided.**

use serde::{Deserialize, Serialize};

use crate::diagnosis::Fix;
use crate::model::{Checks, Person, PrKey};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Add,
    Del,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    #[serde(default)]
    pub no: u32,
    pub kind: LineKind,
    #[serde(default)]
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub lines: Vec<DiffLine>,
}

impl FileDiff {
    /// Builds a diff from `(line number, kind, text)`.
    pub fn new(name: &str, lines: &[(u32, LineKind, &str)]) -> Self {
        Self {
            name: name.into(),
            lines: lines
                .iter()
                .map(|&(no, kind, text)| DiffLine {
                    no,
                    kind,
                    text: text.into(),
                })
                .collect(),
        }
    }

    fn count(&self, kind: LineKind) -> usize {
        self.lines.iter().filter(|l| l.kind == kind).count()
    }

    /// Added and removed lines.
    pub fn counts(&self) -> (usize, usize) {
        (self.count(LineKind::Add), self.count(LineKind::Del))
    }

    /// Short statistics: `+5 −2`.
    pub fn stat(&self) -> String {
        format!(
            "+{} −{}",
            self.count(LineKind::Add),
            self.count(LineKind::Del)
        )
    }
}

/// Weight of a suggestion.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Note,
    Important,
}

impl Level {
    pub fn label_msg(self) -> cockpit_i18n::Msg {
        match self {
            Level::Note => cockpit_i18n::msg!(CoreReviewLevelNote),
            Level::Important => cockpit_i18n::msg!(CoreReviewLevelImportant),
        }
    }

    /// [`Level::label_msg`] in German.
    pub fn label(self) -> String {
        self.label_msg().de()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Approved,
    Declined,
}

/// Which side of the diff a comment sits on (GitHub's `side`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffSide {
    /// Added and unchanged lines (the new file).
    #[default]
    Right,
    /// Removed lines (the old file).
    Left,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl DiffSide {
    fn is_right(&self) -> bool {
        *self == DiffSide::Right
    }

    /// The value GitHub expects: `RIGHT` or `LEFT`.
    pub fn id(self) -> &'static str {
        match self {
            DiffSide::Right => "RIGHT",
            DiffSide::Left => "LEFT",
        }
    }

    /// Reads `RIGHT` or `LEFT` (any case).
    pub fn from_id(id: &str) -> Option<DiffSide> {
        match id.trim().to_ascii_uppercase().as_str() {
            "RIGHT" => Some(DiffSide::Right),
            "LEFT" => Some(DiffSide::Left),
            _ => None,
        }
    }

    /// Does a line of this kind exist on this side?
    pub fn has(self, kind: LineKind) -> bool {
        match self {
            DiffSide::Right => kind != LineKind::Del,
            DiffSide::Left => kind == LineKind::Del,
        }
    }
}

/// Who made a suggestion.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    /// The built-in rules of the demo (no real agent).
    #[default]
    Demo,
    /// The real agent from the container (a draft, checked against the diff).
    Agent,
}

impl Origin {
    fn is_demo(&self) -> bool {
        *self == Origin::Demo
    }
}

/// Comment the agent has prepared. A draft stays on this machine until you approve it and send the
/// review.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    #[serde(default)]
    pub id: String,
    /// File and line, e.g. `components.css · Zeile 417`.
    #[serde(default)]
    pub location: String,
    pub level: Level,
    #[serde(default)]
    pub text: String,
    /// `None` = not decided yet.
    #[serde(default)]
    pub decision: Option<Decision>,
    /// Name of the file in the diff. Empty = no structured position (old data, demo rules): then
    /// only `location` tells where it belongs.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// Line number on `side` (the last line of a range). 0 = unknown.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub line: u32,
    /// First line of a range of lines, on the same side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "DiffSide::is_right")]
    pub side: DiffSide,
    /// Replacement code (like GitHub's „suggestion“ block). Only on the right side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Origin::is_demo")]
    pub origin: Origin,
}

impl Suggestion {
    pub fn new(id: &str, location: &str, level: Level, text: &str) -> Self {
        Self {
            id: id.into(),
            location: location.into(),
            level,
            text: text.into(),
            decision: None,
            path: String::new(),
            line: 0,
            start_line: None,
            side: DiffSide::Right,
            code: None,
            origin: Origin::Demo,
        }
    }

    /// The comment as it goes to GitHub: the text, then the replacement code as a
    /// ```` ```suggestion ````-block.
    pub fn body(&self) -> String {
        with_suggestion(&self.text, self.code.as_deref())
    }
}

/// Text and optional replacement code as one comment body.
pub(crate) fn with_suggestion(text: &str, code: Option<&str>) -> String {
    match code {
        Some(code) if text.is_empty() => format!("```suggestion\n{code}\n```"),
        Some(code) => format!("{text}\n\n```suggestion\n{code}\n```"),
        None => text.to_string(),
    }
}

/// Own comment on a line of the diff. Needs no approval.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LineComment {
    pub id: String,
    /// Name of the file in the diff.
    pub file: String,
    /// Position of the line in `FileDiff::lines`. The line number alone is not enough,
    /// it occurs twice for removed and new lines.
    pub line: usize,
    pub text: String,
    /// Replacement code for the line (like GitHub's „suggestion“ block). May have several
    /// lines. `None` = text only.
    #[serde(default)]
    pub suggestion: Option<String>,
    /// Who wrote the comment. `None` = me. Others' comments are already on
    /// GitHub, they do not go out with the own review and cannot be changed.
    #[serde(default)]
    pub author: Option<Person>,
    /// Key of the agent run that implemented the comment. `None` = still open.
    #[serde(default)]
    pub resolved: Option<String>,
}

impl LineComment {
    /// Written by me.
    pub fn is_own(&self) -> bool {
        self.author.is_none()
    }

    /// Not implemented yet.
    pub fn is_open(&self) -> bool {
        self.resolved.is_none()
    }

    /// The comment as it goes to GitHub: text, then the suggestion as a
    /// ```` ```suggestion ````-Block.
    pub fn body(&self) -> String {
        with_suggestion(&self.text, self.suggestion.as_deref())
    }
}

/// Before/after of a change suggestion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestionDiff {
    /// Line number of the line that is replaced.
    pub no: u32,
    pub before: String,
    pub after: Vec<String>,
}

/// Normalizes a suggestion: without a line break at the end. Empty = no suggestion.
fn clean_suggestion(code: Option<&str>) -> Option<String> {
    let code = code?.trim_end_matches(['\n', '\r']);
    (!code.trim().is_empty()).then(|| code.replace("\r\n", "\n"))
}

/// Result of the review.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verdict {
    #[default]
    Comment,
    Approve,
    RequestChanges,
}

impl Verdict {
    /// The ids of all results (the labels come from [`Verdict::label_msg`]).
    pub const IDS: &'static [&'static str] = &["comment", "approve", "changes"];

    pub fn id(self) -> &'static str {
        match self {
            Verdict::Comment => "comment",
            Verdict::Approve => "approve",
            Verdict::RequestChanges => "changes",
        }
    }

    pub fn label_msg(self) -> cockpit_i18n::Msg {
        match self {
            Verdict::Comment => cockpit_i18n::msg!(CoreReviewVerdictComment),
            Verdict::Approve => cockpit_i18n::msg!(CoreReviewVerdictApprove),
            Verdict::RequestChanges => cockpit_i18n::msg!(CoreReviewVerdictChanges),
        }
    }

    /// [`Verdict::label_msg`] in German.
    pub fn label(self) -> String {
        self.label_msg().de()
    }

    pub fn from_id(id: &str) -> Option<Verdict> {
        [Verdict::Comment, Verdict::Approve, Verdict::RequestChanges]
            .into_iter()
            .find(|v| v.id() == id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    /// This many suggestions are not decided yet.
    Undecided(usize),
    AlreadySent,
    UnknownReview,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Review {
    #[serde(default)]
    pub number: u32,
    /// `owner/name`; empty = unknown (old data, filled by the migration).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// The description as code and parameters (pull requests the agent opens); `None` = only the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_msg: Option<cockpit_i18n::Msg>,
    pub author: Person,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub ticket_key: Option<String>,
    pub checks: Checks,
    #[serde(default)]
    pub files: Vec<FileDiff>,
    /// Has the agent already looked through this pull request?
    #[serde(default)]
    pub agent_reviewed: bool,
    #[serde(default)]
    pub suggestions: Vec<Suggestion>,
    /// What the real agent said about the pull request in its own words. Stays local; it goes out
    /// only if you copy it into your own comment.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_summary: String,
    /// Own comments on diff lines.
    #[serde(default)]
    pub line_comments: Vec<LineComment>,
    #[serde(default)]
    pub verdict: Verdict,
    #[serde(default)]
    pub sent: bool,
    /// The individual checks. Empty as long as there are no details; then only `checks` applies.
    #[serde(default)]
    pub check_runs: Vec<CheckRun>,
    /// `Some` as soon as the pull request is merged – with the chosen method.
    #[serde(default)]
    pub merged: Option<MergeMethod>,
}

impl Review {
    /// Does this pull request belong to the ticket? The agent opened it for the ticket
    /// (`ticket_key`), or its head branch, title or body names the key as a whole word.
    pub fn belongs_to(&self, ticket_key: &str) -> bool {
        self.ticket_key
            .as_deref()
            .is_some_and(|k| k.eq_ignore_ascii_case(ticket_key))
            || crate::link::mentions(&[&self.branch, &self.title, &self.description], ticket_key)
            || self.names_issue(ticket_key)
    }

    /// Does the text of the pull request name the GitHub issue `ticket_key` (`acme/app#12`):
    /// as `#12` (in the pull request's own repo), `acme/app#12` or `Fixes #12`?
    pub fn names_issue(&self, ticket_key: &str) -> bool {
        let Some((repo, number)) = crate::project::parse_issue_key(ticket_key) else {
            return false;
        };
        [&self.branch, &self.title, &self.description]
            .iter()
            .any(|text| {
                crate::project::issue_refs(text, &self.repo)
                    .iter()
                    .any(|r| r.number == number && r.repo.eq_ignore_ascii_case(&repo))
            })
    }

    /// Identity of the pull request: repo and number.
    pub fn key(&self) -> PrKey {
        PrKey::new(&self.repo, self.number)
    }

    fn count(&self, decision: Option<Decision>) -> usize {
        self.suggestions
            .iter()
            .filter(|s| s.decision == decision)
            .count()
    }

    pub fn approved(&self) -> usize {
        self.count(Some(Decision::Approved))
    }

    pub fn declined(&self) -> usize {
        self.count(Some(Decision::Declined))
    }

    pub fn undecided(&self) -> usize {
        self.count(None)
    }

    /// `1 freigegeben · 0 abgelehnt · 2 offen`
    pub fn summary_msg(&self) -> cockpit_i18n::Msg {
        cockpit_i18n::msg!(
            CoreReviewSummary,
            approved = self.approved(),
            declined = self.declined(),
            open = self.undecided()
        )
    }

    /// [`Review::summary_msg`] in German.
    pub fn summary(&self) -> String {
        self.summary_msg().de()
    }

    fn suggestion_mut(&mut self, id: &str) -> Option<&mut Suggestion> {
        self.suggestions.iter_mut().find(|s| s.id == id)
    }

    /// Decides on a suggestion; `None` takes the decision back.
    /// Every change makes a sent review open again.
    pub fn decide(&mut self, id: &str, decision: Option<Decision>) -> bool {
        let Some(s) = self.suggestion_mut(id) else {
            return false;
        };
        s.decision = decision;
        self.sent = false;
        true
    }

    /// Changes the text of a suggestion. Empty text is rejected.
    pub fn edit(&mut self, id: &str, text: &str) -> bool {
        let text = text.trim();
        let Some(s) = self.suggestion_mut(id).filter(|_| !text.is_empty()) else {
            return false;
        };
        s.text = text.to_string();
        self.sent = false;
        true
    }

    pub fn set_verdict(&mut self, verdict: Verdict) {
        self.verdict = verdict;
        self.sent = false;
    }

    // ---------- Merging ----------

    /// Everything that currently prevents merging. Empty = allowed.
    pub fn merge_blockers(&self) -> Vec<MergeBlocker> {
        merge_blockers(self.checks, self.sent, self.verdict, self.merged.is_some())
    }

    /// The agent looks through the diff and prepares suggestions.
    /// Returns how many there turned out to be.
    pub fn run_agent_review(&mut self) -> usize {
        self.suggestions = suggest(&self.files);
        self.agent_reviewed = true;
        self.sent = false;
        self.suggestions.len()
    }
}

mod agent_draft;
mod checks;
mod commit_info;
mod line_comments;
mod merge;
mod sending;
mod suggest;
#[cfg(test)]
mod tests;
mod tree;
mod viewed;

pub use agent_draft::{AgentComment, MAX_AGENT_COMMENTS};
pub use checks::{CHECK_STEPS, CheckRun};
pub use commit_info::{commit_date, short_sha};
pub use merge::{MergeBlocker, MergeMethod, merge_blockers};
pub use suggest::suggest;
pub use tree::{SHOWN_LINES, TreeRow, file_tree};
pub use viewed::Viewed;
