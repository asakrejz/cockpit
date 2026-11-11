//! Pull-Request-Review: Diff, Vorschläge des Agenten, Freigabe und Urteil.
//!
//! Die Kernregel: **Nichts wird gesendet, bevor über jeden Vorschlag entschieden ist.**

use serde::{Deserialize, Serialize};

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
    /// Baut einen Diff aus `(Zeilennummer, Art, Text)`.
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

    /// Kurzstatistik: `+5 −2`.
    pub fn stat(&self) -> String {
        format!(
            "+{} −{}",
            self.count(LineKind::Add),
            self.count(LineKind::Del)
        )
    }
}

/// Gewicht eines Vorschlags.
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

/// Kommentar, den der Agent vorbereitet hat.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    #[serde(default)]
    pub id: String,
    /// Datei und Zeile, z. B. `components.css · Zeile 417`.
    #[serde(default)]
    pub location: String,
    pub level: Level,
    #[serde(default)]
    pub text: String,
    /// `None` = noch nicht entschieden.
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

/// Eigener Kommentar an einer Zeile des Diffs. Braucht keine Freigabe.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LineComment {
    pub id: String,
    /// Name der Datei im Diff.
    pub file: String,
    /// Position der Zeile in `FileDiff::lines`. Die Zeilennummer allein genügt nicht,
    /// sie kommt für entfernte und neue Zeilen doppelt vor.
    pub line: usize,
    pub text: String,
    /// Ersatz-Code für die Zeile (wie GitHubs „suggestion“-Block). Kann mehrere Zeilen
    /// haben. `None` = nur Text.
    #[serde(default)]
    pub suggestion: Option<String>,
}

impl LineComment {
    /// Der Kommentar, wie er an GitHub geht: Text, danach der Vorschlag als
    /// ```` ```suggestion ````-Block.
    pub fn body(&self) -> String {
        match &self.suggestion {
            Some(code) if self.text.is_empty() => format!("```suggestion\n{code}\n```"),
            Some(code) => format!("{}\n\n```suggestion\n{code}\n```", self.text),
            None => self.text.clone(),
        }
    }
}

/// Vorher/Nachher eines Änderungsvorschlags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestionDiff {
    /// Zeilennummer der Zeile, die ersetzt wird.
    pub no: u32,
    pub before: String,
    pub after: Vec<String>,
}

/// Normalisiert einen Vorschlag: ohne Zeilenumbruch am Ende. Leer = kein Vorschlag.
fn clean_suggestion(code: Option<&str>) -> Option<String> {
    let code = code?.trim_end_matches(['\n', '\r']);
    (!code.trim().is_empty()).then(|| code.replace("\r\n", "\n"))
}

/// Ergebnis des Reviews.
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

/// So viele Takte läuft ein neu gestarteter Check in der Demo.
pub const CHECK_STEPS: u8 = 3;

/// Ein einzelner Check eines Pull Requests (Build, Lint, Tests …).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CheckRun {
    pub name: String,
    pub status: Checks,
    /// Dauer des letzten Laufs in Sekunden.
    #[serde(default)]
    pub duration_s: u32,
    /// Ausgabe des letzten Laufs.
    #[serde(default)]
    pub log: String,
    /// Wie ein Neustart in der Demo ausgeht. Fest hinterlegt, damit es nachvollziehbar
    /// bleibt: ein echter Fehler bleibt rot, ein wackliger Test wird grün.
    #[serde(default = "passed")]
    pub rerun: Checks,
    /// Takte, die ein laufender Check noch braucht.
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

    /// Legt fest, wie ein Neustart ausgeht.
    pub fn rerun_as(mut self, rerun: Checks) -> Self {
        self.rerun = rerun;
        self
    }

    /// `2:05 Min.` bzw. `42 Sek.`; während des Laufs `läuft`.
    pub fn duration_label(&self) -> String {
        if self.status == Checks::Running {
            return "läuft".into();
        }
        match self.duration_s {
            s if s < 60 => format!("{s} Sek."),
            s => format!("{}:{:02} Min.", s / 60, s % 60),
        }
    }

    /// Nur ein roter Check lässt sich neu starten.
    fn restart(&mut self) -> bool {
        if self.status != Checks::Failed {
            return false;
        }
        self.status = Checks::Running;
        self.remaining = CHECK_STEPS;
        self.log = format!("$ {} (Neustart)\nwird ausgeführt …\n", self.name);
        true
    }

    /// Ein Takt. `true`, wenn der Check dabei fertig geworden ist.
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

/// Wie ein Pull Request in den Zielbranch kommt.
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

/// Ein Grund, warum noch nicht gemergt werden darf.
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
    /// Begründung für die Oberfläche.
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

/// Die Bedingungen fürs Mergen als reine Funktion: Checks grün, Review gesendet und
/// nicht „Änderungen“. Leer = erlaubt. Ist der Pull Request gemergt, ist das der einzige
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
    /// So viele Vorschläge sind noch nicht entschieden.
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
    /// Hat der Agent diesen Pull Request schon durchgesehen?
    #[serde(default)]
    pub agent_reviewed: bool,
    #[serde(default)]
    pub suggestions: Vec<Suggestion>,
    /// Eigene Kommentare an Diff-Zeilen.
    #[serde(default)]
    pub line_comments: Vec<LineComment>,
    #[serde(default)]
    pub verdict: Verdict,
    #[serde(default)]
    pub sent: bool,
    /// Die einzelnen Checks. Leer, solange es keine Details gibt; dann gilt nur `checks`.
    #[serde(default)]
    pub check_runs: Vec<CheckRun>,
    /// `Some`, sobald der Pull Request gemergt ist – mit der gewählten Methode.
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

    /// Entscheidet über einen Vorschlag; `None` nimmt die Entscheidung zurück.
    /// Jede Änderung macht ein gesendetes Review wieder offen.
    pub fn decide(&mut self, id: &str, decision: Option<Decision>) -> bool {
        let Some(s) = self.suggestion_mut(id) else {
            return false;
        };
        s.decision = decision;
        self.sent = false;
        true
    }

    /// Ändert den Text eines Vorschlags. Leerer Text wird abgelehnt.
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

    // ---------- Eigene Zeilenkommentare ----------

    /// Legt einen Kommentar an der Zeile `line` (Position im Diff) der Datei `file` an.
    /// Optional mit Ersatz-Code für diese Zeile. Gibt die ID zurück.
    ///
    /// Abgelehnt werden: weder Text noch Vorschlag, unbekannte Datei oder Zeile, ein
    /// Vorschlag an einer entfernten Zeile (die gibt es nach dem Merge nicht mehr) und ein
    /// Vorschlag, der die Zeile nicht ändert. Wie jede Änderung macht das ein gesendetes
    /// Review wieder offen.
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

    /// Ändert Text und Vorschlag eines eigenen Kommentars (`None` entfernt den Vorschlag).
    /// Es gelten dieselben Regeln wie beim Anlegen.
    pub fn edit_line_comment(&mut self, id: &str, text: &str, suggestion: Option<&str>) -> bool {
        let text = text.trim();
        let suggestion = clean_suggestion(suggestion);
        let Some(index) = self.line_comments.iter().position(|c| c.id == id) else {
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
        self.line_comments.retain(|c| c.id != id);
        let removed = self.line_comments.len() < before;
        if removed {
            self.sent = false;
        }
        removed
    }

    /// Eigene Kommentare an einer Zeile, in der Reihenfolge des Anlegens.
    pub fn line_comments_at(&self, file: &str, line: usize) -> Vec<&LineComment> {
        self.line_comments
            .iter()
            .filter(|c| c.file == file && c.line == line)
            .collect()
    }

    /// Datei und Zeilennummer eines Kommentars, z. B. `a.css · Zeile 3`.
    pub fn line_location(&self, comment: &LineComment) -> Option<String> {
        let file = self.files.iter().find(|f| f.name == comment.file)?;
        let line = file.lines.get(comment.line)?;
        Some(format!("{} · Zeile {}", file.name, line.no))
    }

    /// Vorher/Nachher eines Änderungsvorschlags; `None`, wenn der Kommentar keinen hat.
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

    /// So viele Kommentare gehen beim Senden raus: freigegebene Vorschläge des Agenten
    /// und alle eigenen Zeilenkommentare.
    pub fn comment_count(&self) -> usize {
        self.approved() + self.line_comments.len()
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

    /// Sendet das Review und gibt die Kommentare zurück: erst die freigegebenen des
    /// Agenten, dann die eigenen Zeilenkommentare.
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
        let own = self.line_comments.iter().map(LineComment::body);
        Ok(approved.chain(own).collect())
    }

    // ---------- Checks ----------

    /// Gesamtstand aus den einzelnen Checks: ein roter genügt für „fehlgeschlagen“,
    /// sonst gilt „laufen“, solange einer läuft. Ohne Details bleibt `checks` wie es ist.
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

    /// Startet einen fehlgeschlagenen Check neu. `false`, wenn es ihn nicht gibt oder er
    /// nicht rot ist.
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

    /// Lässt jeden laufenden Check einen Schritt weiterkommen. Gibt die Namen der Checks
    /// zurück, die dabei fertig geworden sind.
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

    // ---------- Mergen ----------

    /// Alles, was das Mergen gerade verhindert. Leer = erlaubt.
    pub fn merge_blockers(&self) -> Vec<MergeBlocker> {
        merge_blockers(self.checks, self.sent, self.verdict, self.merged.is_some())
    }

    /// Der Agent sieht den Diff durch und bereitet Vorschläge vor.
    /// Gibt zurück, wie viele es geworden sind.
    pub fn run_agent_review(&mut self) -> usize {
        self.suggestions = suggest(&self.files);
        self.agent_reviewed = true;
        self.sent = false;
        self.suggestions.len()
    }
}

/// Einfache Regeln, mit denen der Demo-Agent neue Zeilen prüft. Ein echter Agent
/// würde hier ein Sprachmodell fragen; die Schnittstelle bliebe gleich.
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

/// Erkennt Pixelwerte wie `46px` – außer in Token-Definitionen (`--name: 46px`).
fn has_fixed_px(text: &str) -> bool {
    let is_token_definition = text.trim_start().starts_with("--");
    let has_px = text
        .match_indices("px")
        .any(|(i, _)| text[..i].ends_with(|c: char| c.is_ascii_digit()));
    has_px && !is_token_definition && !text.contains("var(")
}

/// Erkennt Farbwerte wie `#fff` oder `#ffffff`.
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
mod tests {
    use super::LineKind::{Add, Context, Del};
    use super::*;

    fn review() -> Review {
        Review {
            number: 1,
            title: "Titel".into(),
            description: String::new(),
            author: Person::new("Jonas", "JB"),
            branch: "feature/x".into(),
            base: "main".into(),
            ticket_key: None,
            checks: Checks::Passed,
            files: vec![FileDiff::new(
                "a.css",
                &[
                    (1, Context, "x {"),
                    (2, Del, "  a: 1;"),
                    (2, Add, "  color: #ffffff;"),
                    (3, Add, "  outline: none;"),
                ],
            )],
            agent_reviewed: true,
            suggestions: vec![
                Suggestion::new("a", "a.css · Zeile 2", Level::Note, "Erster"),
                Suggestion::new("b", "a.css · Zeile 3", Level::Important, "Zweiter"),
            ],
            line_comments: Vec::new(),
            verdict: Verdict::Comment,
            sent: false,
            check_runs: Vec::new(),
            merged: None,
        }
    }

    #[test]
    fn stat_counts_added_and_removed_lines() {
        assert_eq!(review().files[0].stat(), "+2 −1");
    }

    #[test]
    fn nothing_is_sent_while_a_suggestion_is_undecided() {
        let mut r = review();
        r.decide("a", Some(Decision::Approved));
        assert!(!r.can_send());
        assert_eq!(r.send(), Err(SendError::Undecided(1)));
        assert!(!r.sent);
        assert_eq!(r.summary(), "1 freigegeben · 0 abgelehnt · 1 offen");
    }

    #[test]
    fn only_approved_comments_are_sent() {
        let mut r = review();
        r.decide("a", Some(Decision::Approved));
        r.decide("b", Some(Decision::Declined));
        assert_eq!(r.send_label(), "Review senden · 1 Kommentar");
        assert_eq!(r.send(), Ok(vec!["Erster".to_string()]));
        assert_eq!(r.send_label(), "Gesendet");
        assert_eq!(r.send(), Err(SendError::AlreadySent));
    }

    #[test]
    fn a_review_without_suggestions_can_be_sent() {
        let mut r = review();
        r.suggestions.clear();
        assert_eq!(r.send_label(), "Review senden · 0 Kommentare");
        assert_eq!(r.send(), Ok(vec![]));
    }

    #[test]
    fn edited_text_is_what_gets_sent() {
        let mut r = review();
        assert!(!r.edit("a", "   "), "leerer Text");
        assert!(!r.edit("unbekannt", "x"));
        assert!(r.edit("a", "  Besser so.  "));
        r.decide("a", Some(Decision::Approved));
        r.decide("b", Some(Decision::Declined));
        assert_eq!(r.send(), Ok(vec!["Besser so.".to_string()]));
    }

    #[test]
    fn any_change_reopens_a_sent_review() {
        let mut r = review();
        r.decide("a", Some(Decision::Approved));
        r.decide("b", Some(Decision::Approved));
        r.send().unwrap();
        r.decide("b", None);
        assert!(!r.sent);
        assert_eq!(r.undecided(), 1);

        r.decide("b", Some(Decision::Declined));
        r.send().unwrap();
        r.set_verdict(Verdict::Approve);
        assert!(!r.sent);
    }

    #[test]
    fn agent_review_finds_hard_coded_values() {
        let mut r = review();
        assert_eq!(r.run_agent_review(), 2);
        assert_eq!(r.suggestions[0].location, "a.css · Zeile 2");
        assert_eq!(r.suggestions[0].level, Level::Note);
        assert_eq!(r.suggestions[1].level, Level::Important);
        assert!(r.suggestions.iter().all(|s| s.decision.is_none()));
    }

    #[test]
    fn hex_colors() {
        assert!(has_hex_color("background: #fff;"));
        assert!(has_hex_color("x #12abCD y"));
        assert!(!has_hex_color("PR #9 ist offen"));
        assert!(!has_hex_color("kein Wert"));
    }

    #[test]
    fn fixed_pixel_values() {
        assert!(has_fixed_px("  left: 46px;"));
        assert!(
            !has_fixed_px("  --comp-slider-thumb-w: 38px;"),
            "Tokens dürfen Werte festlegen"
        );
        assert!(!has_fixed_px("  left: var(--sep-inset);"));
        assert!(!has_fixed_px("  display: flex; /* px */"));
    }

    #[test]
    fn verdict_ids() {
        for (id, label) in Verdict::OPTIONS {
            let v = Verdict::from_id(id).unwrap();
            assert_eq!((v.id(), v.label()), (*id, *label));
        }
    }

    #[test]
    fn line_comments_are_added_trimmed_and_found_per_line() {
        let mut r = review();
        assert_eq!(
            r.add_line_comment("a.css", 3, "  Neu  ", None),
            Some("c1".into())
        );
        assert_eq!(
            r.add_line_comment("a.css", 3, "Noch einer", None),
            Some("c2".into())
        );
        assert_eq!(r.line_comments_at("a.css", 3).len(), 2);
        assert_eq!(r.line_comments_at("a.css", 3)[0].text, "Neu");
        assert!(r.line_comments_at("a.css", 2).is_empty());
        let c = r.line_comments[0].clone();
        assert_eq!(r.line_location(&c).as_deref(), Some("a.css · Zeile 3"));
    }

    #[test]
    fn invalid_line_comments_are_rejected() {
        let mut r = review();
        assert_eq!(r.add_line_comment("a.css", 0, "  ", None), None);
        assert_eq!(r.add_line_comment("b.css", 0, "x", None), None);
        assert_eq!(r.add_line_comment("a.css", 4, "x", None), None);
        assert!(r.line_comments.is_empty());
    }

    #[test]
    fn line_comments_can_be_edited_and_deleted() {
        let mut r = review();
        let id = r.add_line_comment("a.css", 1, "Alt", None).unwrap();
        assert!(!r.edit_line_comment(&id, "   ", None));
        assert!(!r.edit_line_comment("nope", "x", None));
        assert!(r.edit_line_comment(&id, " Neu ", None));
        assert_eq!(r.line_comments[0].text, "Neu");
        assert!(!r.delete_line_comment("nope"));
        assert!(r.delete_line_comment(&id));
        assert!(r.line_comments.is_empty());
        // Die ID einer gelöschten Zeile wird nicht doppelt vergeben.
        let a = r.add_line_comment("a.css", 1, "A", None).unwrap();
        let b = r.add_line_comment("a.css", 1, "B", None).unwrap();
        r.delete_line_comment(&a);
        assert_ne!(r.add_line_comment("a.css", 1, "C", None).unwrap(), b);
    }

    #[test]
    fn own_comments_are_sent_and_counted_without_approval() {
        let mut r = review();
        r.add_line_comment("a.css", 2, "Eigener", None);
        assert_eq!(r.comment_count(), 1);
        assert_eq!(r.send_label(), "Review senden · 1 Kommentar");
        // Die Regel bleibt: erst über alle Vorschläge entscheiden.
        assert_eq!(r.send(), Err(SendError::Undecided(2)));
        r.decide("a", Some(Decision::Approved));
        r.decide("b", Some(Decision::Declined));
        assert_eq!(r.send_label(), "Review senden · 2 Kommentare");
        assert_eq!(
            r.send(),
            Ok(vec!["Erster".to_string(), "Eigener".to_string()])
        );
        assert_eq!(r.send_label(), "Gesendet");
    }

    #[test]
    fn a_review_without_suggestions_can_be_sent_with_only_own_comments() {
        let mut r = review();
        r.suggestions.clear();
        r.add_line_comment("a.css", 0, "Nur ich", None);
        assert!(r.can_send());
        assert_eq!(r.send(), Ok(vec!["Nur ich".to_string()]));
    }

    #[test]
    fn changing_line_comments_reopens_a_sent_review() {
        let mut r = review();
        r.suggestions.clear();
        r.send().unwrap();
        assert!(r.sent);
        let id = r.add_line_comment("a.css", 0, "Spät", None).unwrap();
        assert!(!r.sent);
        r.send().unwrap();
        assert!(r.edit_line_comment(&id, "Geändert", None));
        assert!(!r.sent);
        r.send().unwrap();
        assert!(r.delete_line_comment(&id));
        assert!(!r.sent);
        // Misslungene Änderungen öffnen nichts.
        r.send().unwrap();
        assert!(!r.edit_line_comment("nope", "x", None) && !r.delete_line_comment("nope"));
        assert!(r.sent);
    }

    // ---------- Änderungsvorschläge ----------

    #[test]
    fn a_comment_can_carry_a_suggestion() {
        let mut r = review();
        // Zeile 2 (Position 2) ist neu: `  color: #ffffff;`
        let id = r
            .add_line_comment("a.css", 2, "Token nehmen", Some("  color: var(--x);\n"))
            .unwrap();
        let c = r.line_comments[0].clone();
        assert_eq!(c.id, id);
        assert_eq!(c.suggestion.as_deref(), Some("  color: var(--x);"));
        let diff = r.suggestion_diff(&c).unwrap();
        assert_eq!(diff.no, 2);
        assert_eq!(diff.before, "  color: #ffffff;");
        assert_eq!(diff.after, vec!["  color: var(--x);".to_string()]);
        assert_eq!(
            c.body(),
            "Token nehmen\n\n```suggestion\n  color: var(--x);\n```"
        );
    }

    #[test]
    fn a_suggestion_may_replace_one_line_with_several() {
        let mut r = review();
        r.add_line_comment("a.css", 3, "", Some("  a: 1;\n  b: 2;"))
            .unwrap();
        let c = r.line_comments[0].clone();
        assert_eq!(r.suggestion_diff(&c).unwrap().after.len(), 2);
        assert_eq!(c.body(), "```suggestion\n  a: 1;\n  b: 2;\n```");
    }

    #[test]
    fn invalid_suggestions_are_rejected() {
        let mut r = review();
        // Nichts zu sagen.
        assert_eq!(r.add_line_comment("a.css", 2, "  ", Some("  \n")), None);
        // Entfernte Zeile (Position 1): dort lässt sich nichts ersetzen.
        assert_eq!(r.add_line_comment("a.css", 1, "x", Some("neu")), None);
        // Gleicher Inhalt: kein Vorschlag.
        assert_eq!(
            r.add_line_comment("a.css", 3, "x", Some("  outline: none;")),
            None
        );
        assert!(r.line_comments.is_empty());
        // Nur Vorschlag, ohne Text, genügt.
        assert!(r.add_line_comment("a.css", 3, "", Some("x")).is_some());
    }

    #[test]
    fn suggestions_can_be_changed_and_removed() {
        let mut r = review();
        let id = r.add_line_comment("a.css", 2, "T", Some("a")).unwrap();
        assert!(r.edit_line_comment(&id, "T", Some("b")));
        assert_eq!(r.line_comments[0].suggestion.as_deref(), Some("b"));
        assert!(r.edit_line_comment(&id, "T", None));
        assert_eq!(r.line_comments[0].suggestion, None);
        // Ohne Text und ohne Vorschlag bleibt nichts übrig.
        assert!(!r.edit_line_comment(&id, "", None));
        assert!(!r.edit_line_comment(&id, "T", Some("  color: #ffffff;")));
        assert_eq!(r.line_comments[0].text, "T");
    }

    #[test]
    fn the_suggestion_is_sent_as_a_suggestion_block() {
        let mut r = review();
        r.suggestions.clear();
        r.add_line_comment("a.css", 2, "Bitte so", Some("  color: red;"))
            .unwrap();
        let sent = r.send().unwrap();
        assert_eq!(sent, vec!["Bitte so\n\n```suggestion\n  color: red;\n```"]);
    }

    #[test]
    fn old_line_comments_without_the_field_still_load() {
        let c: LineComment =
            serde_json::from_str(r#"{"id":"c1","file":"a","line":0,"text":"x"}"#).unwrap();
        assert_eq!(c.suggestion, None);
    }

    // ---------- Checks ----------

    fn with_checks(r: &mut Review) {
        r.check_runs = vec![
            CheckRun::new("Build", Checks::Passed, 95, "ok"),
            CheckRun::new("Lint", Checks::Failed, 12, "error").rerun_as(Checks::Failed),
            CheckRun::new("Tests", Checks::Failed, 40, "flaky"),
        ];
        r.sync_checks();
    }

    #[test]
    fn the_overall_state_follows_the_single_checks() {
        let mut r = review();
        with_checks(&mut r);
        assert_eq!(r.checks, Checks::Failed);
        assert_eq!(r.check_runs[0].duration_label(), "1:35 Min.");
        assert_eq!(r.check_runs[1].duration_label(), "12 Sek.");
    }

    #[test]
    fn only_a_failed_check_can_be_restarted() {
        let mut r = review();
        with_checks(&mut r);
        assert!(!r.restart_check("Build"), "grün");
        assert!(!r.restart_check("gibt es nicht"));
        assert!(r.restart_check("Tests"));
        assert!(!r.restart_check("Tests"), "läuft schon");
        assert_eq!(r.check_runs[2].status, Checks::Running);
        assert_eq!(r.check_runs[2].duration_label(), "läuft");
        // Lint ist noch rot, also bleibt der Gesamtstand rot.
        assert_eq!(r.checks, Checks::Failed);
    }

    #[test]
    fn a_restarted_check_finishes_after_a_few_ticks_deterministically() {
        let mut r = review();
        with_checks(&mut r);
        r.restart_check("Tests");
        r.restart_check("Lint");
        for _ in 0..CHECK_STEPS - 1 {
            assert!(r.advance_checks().is_empty());
            assert_eq!(r.checks, Checks::Running, "rot ist nur, wer fertig ist");
        }
        assert_eq!(r.advance_checks(), vec!["Lint", "Tests"]);
        assert_eq!(r.check_runs[2].status, Checks::Passed);
        assert!(r.check_runs[2].log.contains("erfolgreich"));
        assert_eq!(r.check_runs[1].status, Checks::Failed);
        assert_eq!(r.checks, Checks::Failed);
        assert_eq!(r.running_checks(), 0);
        assert!(r.advance_checks().is_empty());
        // Der echte Fehler lässt sich erneut versuchen, bleibt aber rot.
        assert!(r.restart_check("Lint"));
    }

    #[test]
    fn all_green_after_restarts_makes_the_review_green() {
        let mut r = review();
        r.check_runs = vec![
            CheckRun::new("Build", Checks::Passed, 5, ""),
            CheckRun::new("Tests", Checks::Failed, 5, ""),
        ];
        r.restart_check("Tests");
        (0..CHECK_STEPS).for_each(|_| {
            r.advance_checks();
        });
        assert_eq!(r.checks, Checks::Passed);
    }

    // ---------- Mergen ----------

    #[test]
    fn merging_needs_green_checks_a_sent_review_and_no_requested_changes() {
        use MergeBlocker::*;
        let ok = |c, sent, v| merge_blockers(c, sent, v, false);
        assert_eq!(ok(Checks::Passed, true, Verdict::Approve), vec![]);
        assert_eq!(ok(Checks::Passed, true, Verdict::Comment), vec![]);
        assert_eq!(
            ok(Checks::Failed, true, Verdict::Approve),
            vec![ChecksFailed]
        );
        assert_eq!(
            ok(Checks::Running, true, Verdict::Approve),
            vec![ChecksRunning]
        );
        assert_eq!(
            ok(Checks::Passed, false, Verdict::Approve),
            vec![ReviewNotSent]
        );
        assert_eq!(
            ok(Checks::Passed, true, Verdict::RequestChanges),
            vec![ChangesRequested]
        );
        assert_eq!(
            ok(Checks::Failed, false, Verdict::RequestChanges),
            vec![ChecksFailed, ReviewNotSent, ChangesRequested]
        );
        assert_eq!(
            merge_blockers(Checks::Passed, true, Verdict::Approve, true),
            vec![AlreadyMerged]
        );
    }

    #[test]
    fn every_blocker_has_a_german_reason() {
        use MergeBlocker::*;
        for b in [
            AlreadyMerged,
            Draft,
            ChecksFailed,
            ChecksRunning,
            ReviewNotSent,
            ChangesRequested,
        ] {
            assert!(b.reason().ends_with('.'), "{b:?}");
        }
    }

    #[test]
    fn merge_methods_have_ids() {
        for (id, label) in MergeMethod::OPTIONS {
            let m = MergeMethod::from_id(id).unwrap();
            assert_eq!((m.id(), m.label()), (*id, *label));
        }
        assert_eq!(MergeMethod::from_id("x"), None);
    }
}
