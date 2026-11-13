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
    pub fn label(self) -> &'static str {
        match self {
            Level::Note => "Hinweis",
            Level::Important => "Wichtig",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Approved,
    Declined,
}

/// Comment the agent has prepared.
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
}

impl Suggestion {
    pub fn new(id: &str, location: &str, level: Level, text: &str) -> Self {
        Self {
            id: id.into(),
            location: location.into(),
            level,
            text: text.into(),
            decision: None,
        }
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
        match &self.suggestion {
            Some(code) if self.text.is_empty() => format!("```suggestion\n{code}\n```"),
            Some(code) => format!("{}\n\n```suggestion\n{code}\n```", self.text),
            None => self.text.clone(),
        }
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
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("comment", "Kommentieren"),
        ("approve", "Genehmigen"),
        ("changes", "Änderungen"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            Verdict::Comment => "comment",
            Verdict::Approve => "approve",
            Verdict::RequestChanges => "changes",
        }
    }

    pub fn label(self) -> &'static str {
        Self::OPTIONS
            .iter()
            .find(|(id, _)| *id == self.id())
            .map_or("", |(_, label)| label)
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
    pub fn summary(&self) -> String {
        format!(
            "{} freigegeben · {} abgelehnt · {} offen",
            self.approved(),
            self.declined(),
            self.undecided()
        )
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

    // ---------- Own line comments ----------

    /// Creates a comment on the line `line` (position in the diff) of the file `file`.
    /// Optionally with replacement code for this line. Returns the ID.
    ///
    /// Rejected are: neither text nor suggestion, unknown file or line, a
    /// suggestion on a removed line (it no longer exists after the merge) and a
    /// suggestion that does not change the line. Like every change this makes a sent
    /// review open again.
    pub fn add_line_comment(
        &mut self,
        file: &str,
        line: usize,
        text: &str,
        suggestion: Option<&str>,
    ) -> Option<String> {
        let text = text.trim();
        let suggestion = clean_suggestion(suggestion);
        let diff_line = self
            .files
            .iter()
            .find(|f| f.name == file)
            .and_then(|f| f.lines.get(line))?;
        if (text.is_empty() && suggestion.is_none())
            || !Self::suggestion_fits(diff_line, suggestion.as_deref())
        {
            return None;
        }
        let next = self
            .line_comments
            .iter()
            .filter_map(|c| c.id.strip_prefix('c')?.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let id = format!("c{next}");
        self.line_comments.push(LineComment {
            id: id.clone(),
            file: file.into(),
            line,
            text: text.into(),
            suggestion,
            author: None,
            resolved: None,
        });
        self.sent = false;
        Some(id)
    }

    fn suggestion_fits(line: &DiffLine, suggestion: Option<&str>) -> bool {
        match suggestion {
            None => true,
            Some(code) => line.kind != LineKind::Del && code != line.text,
        }
    }

    /// Changes text and suggestion of an own comment (`None` removes the suggestion).
    /// The same rules apply as when creating.
    pub fn edit_line_comment(&mut self, id: &str, text: &str, suggestion: Option<&str>) -> bool {
        let text = text.trim();
        let suggestion = clean_suggestion(suggestion);
        let Some(index) = self
            .line_comments
            .iter()
            .position(|c| c.id == id && c.is_own())
        else {
            return false;
        };
        let comment = &self.line_comments[index];
        let fits = self
            .files
            .iter()
            .find(|f| f.name == comment.file)
            .and_then(|f| f.lines.get(comment.line))
            .is_some_and(|l| Self::suggestion_fits(l, suggestion.as_deref()));
        if (text.is_empty() && suggestion.is_none()) || !fits {
            return false;
        }
        let c = &mut self.line_comments[index];
        c.text = text.to_string();
        c.suggestion = suggestion;
        self.sent = false;
        true
    }

    pub fn delete_line_comment(&mut self, id: &str) -> bool {
        let before = self.line_comments.len();
        self.line_comments.retain(|c| c.id != id || !c.is_own());
        let removed = self.line_comments.len() < before;
        if removed {
            self.sent = false;
        }
        removed
    }

    // ---------- Implementing comments (follow-up job for the agent) ----------

    /// Comments that are not implemented yet – own as well as others'.
    pub fn open_comments(&self) -> Vec<&LineComment> {
        self.line_comments.iter().filter(|c| c.is_open()).collect()
    }

    /// Sets the comments to „done“, with a reference to the run. Returns how
    /// many there were; already done and unknown ones do not count. This changes nothing about
    /// the verdict, but the review then counts as to be sent anew.
    pub fn resolve_comments(&mut self, ids: &[String], run: &str) -> usize {
        let mut n = 0;
        for c in self
            .line_comments
            .iter_mut()
            .filter(|c| c.is_open() && ids.contains(&c.id))
        {
            c.resolved = Some(run.to_string());
            n += 1;
        }
        n
    }

    /// Takes over the change suggestion of a comment into the diff: the line is replaced by
    /// the replacement. An unchanged line stays as a removed one. Comments
    /// further down in the file move along. `false` if there is nothing to take over.
    pub fn apply_comment_suggestion(&mut self, id: &str) -> bool {
        let Some((file, index, code)) = self
            .line_comments
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| Some((c.file.clone(), c.line, c.suggestion.clone()?)))
        else {
            return false;
        };
        let Some(diff) = self.files.iter_mut().find(|f| f.name == file) else {
            return false;
        };
        let Some(old) = diff.lines.get(index).cloned() else {
            return false;
        };
        if old.kind == LineKind::Del {
            return false;
        }
        let mut replacement = Vec::new();
        if old.kind == LineKind::Context {
            replacement.push(DiffLine {
                kind: LineKind::Del,
                ..old.clone()
            });
        }
        replacement.extend(code.lines().enumerate().map(|(i, text)| DiffLine {
            no: old.no + i as u32,
            kind: LineKind::Add,
            text: text.to_string(),
        }));
        let grown = replacement.len() - 1;
        diff.lines.splice(index..=index, replacement);
        self.shift_comments(&file, index, grown as isize);
        true
    }

    /// Moves the comments below `index` by `by` lines.
    fn shift_comments(&mut self, file: &str, index: usize, by: isize) {
        for c in self
            .line_comments
            .iter_mut()
            .filter(|c| c.file == file && c.line > index)
        {
            c.line = c.line.saturating_add_signed(by);
        }
    }

    /// Applies the change of a diagnosis: the line is replaced or removed.
    /// `false` if file or line (no longer) fit.
    pub fn apply_fix(&mut self, fix: &Fix) -> bool {
        let Some(diff) = self.files.iter_mut().find(|f| f.name == fix.file) else {
            return false;
        };
        let Some(index) = diff.lines.iter().position(|l| {
            l.no == fix.no && l.kind != LineKind::Del && l.text.trim_end() == fix.before.trim_end()
        }) else {
            return false;
        };
        let file = fix.file.clone();
        match &fix.after {
            Some(after) => diff.lines[index].text = after.clone(),
            None => {
                diff.lines.remove(index);
                self.shift_comments(&file, index, -1);
            }
        }
        true
    }

    /// Determines how the next restart of a check ends (after a fix: green).
    pub fn set_rerun(&mut self, check: &str, rerun: Checks) {
        if let Some(c) = self.check_runs.iter_mut().find(|c| c.name == check) {
            c.rerun = rerun;
        }
    }

    /// Own comments on a line, in the order of creation.
    pub fn line_comments_at(&self, file: &str, line: usize) -> Vec<&LineComment> {
        self.line_comments
            .iter()
            .filter(|c| c.file == file && c.line == line)
            .collect()
    }

    /// File and line number of a comment, e.g. `a.css · Zeile 3`.
    pub fn line_location(&self, comment: &LineComment) -> Option<String> {
        let file = self.files.iter().find(|f| f.name == comment.file)?;
        let line = file.lines.get(comment.line)?;
        Some(format!("{} · Zeile {}", file.name, line.no))
    }

    /// Before/after of a change suggestion; `None` if the comment has none.
    pub fn suggestion_diff(&self, comment: &LineComment) -> Option<SuggestionDiff> {
        let code = comment.suggestion.as_ref()?;
        let line = self
            .files
            .iter()
            .find(|f| f.name == comment.file)?
            .lines
            .get(comment.line)?;
        Some(SuggestionDiff {
            no: line.no,
            before: line.text.clone(),
            after: code.lines().map(str::to_string).collect(),
        })
    }

    /// Own line comments that go out on sending: others' are already on GitHub,
    /// implemented ones are done.
    fn outgoing(&self) -> impl Iterator<Item = &LineComment> {
        self.line_comments
            .iter()
            .filter(|c| c.is_own() && c.is_open())
    }

    /// This many comments go out on sending: approved suggestions of the agent
    /// and the open own line comments.
    pub fn comment_count(&self) -> usize {
        self.approved() + self.outgoing().count()
    }

    pub fn can_send(&self) -> bool {
        !self.sent && self.undecided() == 0
    }

    pub fn send_label(&self) -> String {
        if self.sent {
            return "Gesendet".into();
        }
        let n = self.comment_count();
        format!(
            "Review senden · {n} {}",
            if n == 1 { "Kommentar" } else { "Kommentare" }
        )
    }

    /// Sends the review and returns the comments: first the approved ones of the
    /// agent, then the own line comments.
    pub fn send(&mut self) -> Result<Vec<String>, SendError> {
        if self.sent {
            return Err(SendError::AlreadySent);
        }
        if self.undecided() > 0 {
            return Err(SendError::Undecided(self.undecided()));
        }
        self.sent = true;
        let approved = self
            .suggestions
            .iter()
            .filter(|s| s.decision == Some(Decision::Approved))
            .map(|s| s.text.clone());
        let own = self.outgoing().map(LineComment::body);
        Ok(approved.chain(own).collect())
    }

    // ---------- Checks ----------

    /// Overall state from the individual checks: one red is enough for „failed“,
    /// otherwise „running“ applies as long as one is running. Without details `checks` stays as it is.
    pub fn sync_checks(&mut self) {
        if self.check_runs.is_empty() {
            return;
        }
        let any = |c: Checks| self.check_runs.iter().any(|r| r.status == c);
        self.checks = if any(Checks::Failed) {
            Checks::Failed
        } else if any(Checks::Running) {
            Checks::Running
        } else {
            Checks::Passed
        };
    }

    /// Restarts a failed check. `false` if it does not exist or is
    /// not red.
    pub fn restart_check(&mut self, name: &str) -> bool {
        let restarted = self
            .check_runs
            .iter_mut()
            .find(|c| c.name == name)
            .is_some_and(CheckRun::restart);
        if restarted {
            self.sync_checks();
        }
        restarted
    }

    /// Lets every running check advance one step. Returns the names of the checks
    /// that finished in the process.
    pub fn advance_checks(&mut self) -> Vec<String> {
        let done: Vec<String> = self
            .check_runs
            .iter_mut()
            .filter_map(|c| c.advance().then(|| c.name.clone()))
            .collect();
        if !done.is_empty() {
            self.sync_checks();
        }
        done
    }

    pub fn running_checks(&self) -> usize {
        self.check_runs
            .iter()
            .filter(|c| c.status == Checks::Running)
            .count()
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

mod checks;
mod merge;
mod suggest;
#[cfg(test)]
mod tests;

pub use checks::{CHECK_STEPS, CheckRun};
pub use merge::{MergeBlocker, MergeMethod, merge_blockers};
pub use suggest::suggest;
