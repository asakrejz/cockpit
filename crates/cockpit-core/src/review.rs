//! Pull-Request-Review: Diff, Vorschläge des Agenten, Freigabe und Urteil.
//!
//! Die Kernregel: **Nichts wird gesendet, bevor über jeden Vorschlag entschieden ist.**

use crate::model::{Checks, Person};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Add,
    Del,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub no: u32,
    pub kind: LineKind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    pub name: String,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Approved,
    Declined,
}

/// Kommentar, den der Agent vorbereitet hat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub id: String,
    /// Datei und Zeile, z. B. `components.css · Zeile 417`.
    pub location: String,
    pub level: Level,
    pub text: String,
    /// `None` = noch nicht entschieden.
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineComment {
    pub id: String,
    /// Name der Datei im Diff.
    pub file: String,
    /// Position der Zeile in `FileDiff::lines`. Die Zeilennummer allein genügt nicht,
    /// sie kommt für entfernte und neue Zeilen doppelt vor.
    pub line: usize,
    pub text: String,
}

/// Ergebnis des Reviews.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    /// So viele Vorschläge sind noch nicht entschieden.
    Undecided(usize),
    AlreadySent,
    UnknownReview,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    pub number: u32,
    pub title: String,
    pub description: String,
    pub author: Person,
    pub branch: String,
    pub base: String,
    pub ticket_key: Option<String>,
    pub checks: Checks,
    pub files: Vec<FileDiff>,
    /// Hat der Agent diesen Pull Request schon durchgesehen?
    pub agent_reviewed: bool,
    pub suggestions: Vec<Suggestion>,
    /// Eigene Kommentare an Diff-Zeilen.
    pub line_comments: Vec<LineComment>,
    pub verdict: Verdict,
    pub sent: bool,
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
    /// Gibt die ID zurück. Leerer Text, unbekannte Datei oder Zeile werden abgelehnt.
    /// Wie jede Änderung macht das ein gesendetes Review wieder offen.
    pub fn add_line_comment(&mut self, file: &str, line: usize, text: &str) -> Option<String> {
        let text = text.trim();
        let exists = self
            .files
            .iter()
            .any(|f| f.name == file && line < f.lines.len());
        if text.is_empty() || !exists {
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
        });
        self.sent = false;
        Some(id)
    }

    /// Ändert den Text eines eigenen Kommentars. Leerer Text wird abgelehnt.
    pub fn edit_line_comment(&mut self, id: &str, text: &str) -> bool {
        let text = text.trim();
        let Some(c) = self
            .line_comments
            .iter_mut()
            .find(|c| c.id == id)
            .filter(|_| !text.is_empty())
        else {
            return false;
        };
        c.text = text.to_string();
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
        let own = self.line_comments.iter().map(|c| c.text.clone());
        Ok(approved.chain(own).collect())
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
        assert_eq!(r.add_line_comment("a.css", 3, "  Neu  "), Some("c1".into()));
        assert_eq!(
            r.add_line_comment("a.css", 3, "Noch einer"),
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
        assert_eq!(r.add_line_comment("a.css", 0, "  "), None);
        assert_eq!(r.add_line_comment("b.css", 0, "x"), None);
        assert_eq!(r.add_line_comment("a.css", 4, "x"), None);
        assert!(r.line_comments.is_empty());
    }

    #[test]
    fn line_comments_can_be_edited_and_deleted() {
        let mut r = review();
        let id = r.add_line_comment("a.css", 1, "Alt").unwrap();
        assert!(!r.edit_line_comment(&id, "   "));
        assert!(!r.edit_line_comment("nope", "x"));
        assert!(r.edit_line_comment(&id, " Neu "));
        assert_eq!(r.line_comments[0].text, "Neu");
        assert!(!r.delete_line_comment("nope"));
        assert!(r.delete_line_comment(&id));
        assert!(r.line_comments.is_empty());
        // Die ID einer gelöschten Zeile wird nicht doppelt vergeben.
        let a = r.add_line_comment("a.css", 1, "A").unwrap();
        let b = r.add_line_comment("a.css", 1, "B").unwrap();
        r.delete_line_comment(&a);
        assert_ne!(r.add_line_comment("a.css", 1, "C").unwrap(), b);
    }

    #[test]
    fn own_comments_are_sent_and_counted_without_approval() {
        let mut r = review();
        r.add_line_comment("a.css", 2, "Eigener");
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
        r.add_line_comment("a.css", 0, "Nur ich");
        assert!(r.can_send());
        assert_eq!(r.send(), Ok(vec!["Nur ich".to_string()]));
    }

    #[test]
    fn changing_line_comments_reopens_a_sent_review() {
        let mut r = review();
        r.suggestions.clear();
        r.send().unwrap();
        assert!(r.sent);
        let id = r.add_line_comment("a.css", 0, "Spät").unwrap();
        assert!(!r.sent);
        r.send().unwrap();
        assert!(r.edit_line_comment(&id, "Geändert"));
        assert!(!r.sent);
        r.send().unwrap();
        assert!(r.delete_line_comment(&id));
        assert!(!r.sent);
        // Misslungene Änderungen öffnen nichts.
        r.send().unwrap();
        assert!(!r.edit_line_comment("nope", "x") && !r.delete_line_comment("nope"));
        assert!(r.sent);
    }
}
