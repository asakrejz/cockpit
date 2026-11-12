//! Pull request review: diff, suggestions of the agent, approval and verdict.
//!
//! The core rule: **Nothing is sent before every suggestion has been decided.**

use serde::{Deserialize, Serialize};

use crate::diagnosis::Fix;
use crate::model::{Checks, Person};

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

/// This many ticks a restarted check runs in the demo.
pub const CHECK_STEPS: u8 = 3;

/// A single check of a pull request (build, lint, tests …).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CheckRun {
    pub name: String,
    pub status: Checks,
    /// Duration of the last run in seconds.
    #[serde(default)]
    pub duration_s: u32,
    /// Output of the last run.
    #[serde(default)]
    pub log: String,
    /// How a restart ends in the demo. Fixed so that it stays traceable:
    /// a real error stays red, a flaky test turns green.
    #[serde(default = "passed")]
    pub rerun: Checks,
    /// Ticks a running check still needs.
    #[serde(default)]
    pub remaining: u8,
}

fn passed() -> Checks {
    Checks::Passed
}

impl CheckRun {
    pub fn new(name: &str, status: Checks, duration_s: u32, log: &str) -> Self {
        Self {
            name: name.into(),
            status,
            duration_s,
            log: log.into(),
            rerun: Checks::Passed,
            remaining: 0,
        }
    }

    /// Determines how a restart ends.
    pub fn rerun_as(mut self, rerun: Checks) -> Self {
        self.rerun = rerun;
        self
    }

    /// `2:05 Min.` or `42 Sek.`; while running `läuft`.
    pub fn duration_label(&self) -> String {
        if self.status == Checks::Running {
            return "läuft".into();
        }
        match self.duration_s {
            s if s < 60 => format!("{s} Sek."),
            s => format!("{}:{:02} Min.", s / 60, s % 60),
        }
    }

    /// Only a red check can be restarted.
    fn restart(&mut self) -> bool {
        if self.status != Checks::Failed {
            return false;
        }
        self.status = Checks::Running;
        self.remaining = CHECK_STEPS;
        self.log = format!("$ {} (Neustart)\nwird ausgeführt …\n", self.name);
        true
    }

    /// One tick. `true` if the check finished in the process.
    fn advance(&mut self) -> bool {
        if self.status != Checks::Running {
            return false;
        }
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining > 0 {
            return false;
        }
        self.status = self.rerun;
        self.log = match self.rerun {
            Checks::Failed => format!(
                "$ {} (Neustart)\nerror: derselbe Fehler wie zuvor – der Check bleibt rot.\n",
                self.name
            ),
            _ => format!("$ {} (Neustart)\nalle Schritte erfolgreich\n", self.name),
        };
        true
    }
}

/// How a pull request gets into the target branch.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergeMethod {
    #[default]
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("merge", "Merge"),
        ("squash", "Squash"),
        ("rebase", "Rebase"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            MergeMethod::Merge => "merge",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
        }
    }

    pub fn label(self) -> &'static str {
        Self::OPTIONS
            .iter()
            .find(|(id, _)| *id == self.id())
            .map_or("", |(_, label)| label)
    }

    pub fn from_id(id: &str) -> Option<MergeMethod> {
        [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase]
            .into_iter()
            .find(|m| m.id() == id)
    }
}

/// A reason why merging is not allowed yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeBlocker {
    AlreadyMerged,
    Draft,
    ChecksFailed,
    ChecksRunning,
    ReviewNotSent,
    ChangesRequested,
}

impl MergeBlocker {
    /// Justification for the UI.
    pub fn reason(self) -> &'static str {
        match self {
            MergeBlocker::AlreadyMerged => "Der Pull Request ist schon gemergt.",
            MergeBlocker::Draft => "Der Pull Request ist noch ein Entwurf.",
            MergeBlocker::ChecksFailed => "Mindestens ein Check ist fehlgeschlagen.",
            MergeBlocker::ChecksRunning => "Die Checks laufen noch.",
            MergeBlocker::ReviewNotSent => "Dein Review ist noch nicht gesendet.",
            MergeBlocker::ChangesRequested => {
                "Du hast „Änderungen“ angefordert – erst das Ergebnis ändern."
            }
        }
    }
}

/// The conditions for merging as a pure function: checks green, review sent and
/// not „Changes“. Empty = allowed. If the pull request is merged, that is the only
/// Grund.
pub fn merge_blockers(
    checks: Checks,
    review_sent: bool,
    verdict: Verdict,
    merged: bool,
) -> Vec<MergeBlocker> {
    if merged {
        return vec![MergeBlocker::AlreadyMerged];
    }
    let mut out = Vec::new();
    match checks {
        Checks::Failed => out.push(MergeBlocker::ChecksFailed),
        Checks::Running => out.push(MergeBlocker::ChecksRunning),
        Checks::Passed => {}
    }
    if !review_sent {
        out.push(MergeBlocker::ReviewNotSent);
    }
    if verdict == Verdict::RequestChanges {
        out.push(MergeBlocker::ChangesRequested);
    }
    out
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

/// Simple rules with which the demo agent checks new lines. A real agent
/// would ask a language model here; the interface would stay the same.
pub fn suggest(files: &[FileDiff]) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for file in files {
        for line in file.lines.iter().filter(|l| l.kind == LineKind::Add) {
            let text = line.text.as_str();
            let finding = if text.contains("outline: none") {
                Some((
                    Level::Important,
                    "outline: none entfernt den Fokus-Ring ohne Ersatz. Wer mit der Tastatur bedient, sieht den Fokus nicht mehr – bitte --sys-focus-ring setzen.",
                ))
            } else if has_hex_color(text) {
                Some((
                    Level::Note,
                    "Hier steht ein fester Farbwert. Komponenten sollen nur Tokens lesen – bitte eine --sys- oder --comp-Variable verwenden.",
                ))
            } else if text.contains("transition") && !text.contains("var(") {
                Some((
                    Level::Note,
                    "Die Dauer ist fest eingetragen. Mit var(--sys-motion-snappy) greift auch die Einstellung „Bewegung reduzieren“.",
                ))
            } else if has_fixed_px(text) {
                Some((
                    Level::Note,
                    "Fester Pixelwert. Bitte aus den Tokens ableiten (--ref-space-*, --comp-*), damit der Wert mit dem Design mitwächst.",
                ))
            } else {
                None
            };
            if let Some((level, message)) = finding {
                let id = format!("s{}", out.len() + 1);
                out.push(Suggestion::new(
                    &id,
                    &format!("{} · Zeile {}", file.name, line.no),
                    level,
                    message,
                ));
            }
        }
    }
    out
}

/// Detects pixel values like `46px` – except in token definitions (`--name: 46px`).
fn has_fixed_px(text: &str) -> bool {
    let is_token_definition = text.trim_start().starts_with("--");
    let has_px = text
        .match_indices("px")
        .any(|(i, _)| text[..i].ends_with(|c: char| c.is_ascii_digit()));
    has_px && !is_token_definition && !text.contains("var(")
}

/// Detects color values like `#fff` or `#ffffff`.
fn has_hex_color(text: &str) -> bool {
    text.match_indices('#').any(|(i, _)| {
        text[i + 1..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .count()
            >= 3
    })
}

#[cfg(test)]
mod tests;
