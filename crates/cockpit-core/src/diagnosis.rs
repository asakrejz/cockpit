//! Diagnosis of a red check: the agent reads the log and presents cause and proposal.
//!
//! [`diagnose`] is a pure function of review and check. The demo recognizes a few
//! typical errors in the log (unused variable or import, failed test,
//! timeout) and stays deterministic; a real agent would ask a
//! language model here, the shape of the result would stay the same.
//!
//! None of this changes the branch. Only „Fix übernehmen“ (a separate step of the run)
//! applies the [`Fix`].

use serde::{Deserialize, Serialize};

use crate::review::{CheckRun, DiffLine, FileDiff, LineKind, Review};

/// A change to a single line of the pull request.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    /// Name of the file in the diff.
    pub file: String,
    /// Line number of the line that changes.
    pub no: u32,
    /// Content of the line before the change.
    pub before: String,
    /// New content; `None` = the line is dropped.
    pub after: Option<String>,
}

/// What the agent found out about a red check.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Diagnosis {
    /// Name of the check.
    pub check: String,
    /// The cause in one sentence.
    pub cause: String,
    /// Affected file and line, as far as the log names them.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    /// What the agent proposes, in one sentence.
    pub remedy: String,
    /// The proposal as a diff (line with surroundings). `None` if there is no code to change.
    #[serde(default)]
    pub patch: Option<FileDiff>,
    /// The change that „Fix übernehmen“ brings onto the branch.
    #[serde(default)]
    pub fix: Option<Fix>,
}

impl Diagnosis {
    /// `list.rs · Zeile 62`
    pub fn location(&self) -> Option<String> {
        let file = self.file.as_ref()?;
        Some(match self.line {
            Some(line) => format!("{file} · Zeile {line}"),
            None => file.clone(),
        })
    }

    /// Does the proposal change code? Otherwise it is enough to restart the check.
    pub fn changes_code(&self) -> bool {
        self.fix.is_some()
    }
}

/// The first real error message of the log (without the follow-up line „could not compile“).
fn first_error(log: &str) -> Option<&str> {
    log.lines().map(str::trim_start).find_map(|l| {
        let rest = l.strip_prefix("error")?;
        // `error: …` or `error[E0308]: …`
        let msg = rest
            .split_once("]: ")
            .map(|(_, m)| m)
            .or_else(|| rest.strip_prefix(": "))?;
        (!msg.starts_with("could not compile") && !msg.starts_with("aborting")).then_some(msg)
    })
}

/// `--> pfad/datei.rs:62:13` → `("pfad/datei.rs", 62)`
fn location_of(log: &str) -> Option<(String, u32)> {
    let rest = log
        .lines()
        .find_map(|l| l.trim_start().strip_prefix("--> "))?;
    let mut parts = rest.trim().rsplitn(3, ':');
    let _column = parts.next()?;
    let line = parts.next()?.parse().ok()?;
    Some((parts.next()?.to_string(), line))
}

/// The source line `62 |   code` that the log shows for a line number.
fn excerpt_of(log: &str, line: u32) -> Option<String> {
    log.lines().find_map(|l| {
        let (no, code) = l.split_once('|')?;
        (no.trim().parse::<u32>().ok()? == line)
            .then(|| code.strip_prefix(' ').unwrap_or(code).to_string())
    })
}

/// The name in backticks: ``unused variable: `inset` `` → `inset`.
fn quoted(message: &str) -> Option<&str> {
    let (_, rest) = message.split_once('`')?;
    Some(rest.split_once('`')?.0)
}

/// Line `index` with one line of surroundings each, the line itself as removed or replaced.
fn patch_around(file: &FileDiff, index: usize, after: Option<&str>) -> FileDiff {
    let from = index.saturating_sub(1);
    let to = (index + 2).min(file.lines.len());
    let mut lines = Vec::new();
    for (i, l) in file.lines[from..to].iter().enumerate() {
        if from + i != index {
            lines.push(DiffLine {
                kind: LineKind::Context,
                ..l.clone()
            });
            continue;
        }
        lines.push(DiffLine {
            kind: LineKind::Del,
            ..l.clone()
        });
        if let Some(text) = after {
            lines.push(DiffLine {
                no: l.no,
                kind: LineKind::Add,
                text: text.to_string(),
            });
        }
    }
    FileDiff {
        name: file.name.clone(),
        lines,
    }
}

/// Reads the log of a red check and presents a diagnosis.
///
/// * Unused variable or unused import that the pull request adds itself:
///   the proposal is to remove the line.
/// * Timeout without an error message: not a code error, the check is flaky – the proposal
///   is a restart.
/// * Failed test or unknown error: the cause from the log, without a proposal
///   for the code.
pub fn diagnose(review: &Review, check: &CheckRun) -> Diagnosis {
    let log = check.log.as_str();
    let mut d = Diagnosis {
        check: check.name.clone(),
        cause: String::new(),
        file: None,
        line: None,
        remedy: "Check neu starten".into(),
        patch: None,
        fix: None,
    };

    if let Some(message) = first_error(log) {
        let place = location_of(log);
        if let Some((path, line)) = &place {
            d.file = Some(path.rsplit('/').next().unwrap_or(path).to_string());
            d.line = Some(*line);
        }
        let kind = ["variable", "import"]
            .into_iter()
            .find(|k| message.starts_with(&format!("unused {k}")));
        if let (Some(kind), Some(name)) = (kind, quoted(message)) {
            let noun = if kind == "variable" {
                "Variable"
            } else {
                "Import"
            };
            d.cause = format!(
                "Ungenutzt{} {noun} `{name}`: Mit `-D warnings` bricht der Lint-Lauf dort ab.",
                if kind == "variable" { "e" } else { "er" }
            );
            d.remedy = "Die ungenutzte Zeile entfernen".into();
            // The fix only applies if the line is in the pull request and matches the log.
            if let (Some(file), Some(line)) = (d.file.as_deref(), d.line)
                && let Some(code) = excerpt_of(log, line)
                && let Some(diff) = review.files.iter().find(|f| f.name == file)
                && let Some(index) = diff.lines.iter().position(|l| {
                    l.no == line && l.kind != LineKind::Del && l.text.trim() == code.trim()
                })
            {
                d.patch = Some(patch_around(diff, index, None));
                d.fix = Some(Fix {
                    file: file.to_string(),
                    no: line,
                    before: diff.lines[index].text.clone(),
                    after: None,
                });
                d.remedy = format!("Zeile {line} in {file} entfernen");
            }
        } else {
            d.cause = format!("Der Lauf meldet: {message}");
            d.remedy = "Ursache im Code prüfen – dafür gibt es keinen automatischen Fix".into();
        }
        return d;
    }

    if let Some(test) = log.lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("test ")?.strip_suffix(" ... FAILED")
    }) {
        let reason = log
            .lines()
            .find_map(|l| l.trim().strip_prefix("thread panicked: "))
            .unwrap_or("siehe Log");
        // A timeout when starting the environment says nothing about the code.
        if reason.contains("timeout") {
            d.cause = format!(
                "Zeitüberschreitung bei `{test}`: Der Headless-Browser ist nicht rechtzeitig gestartet – das sieht nach einem wackligen Check aus, nicht nach einem Fehler im Code."
            );
            d.remedy = "Kein Code-Fix nötig – Check neu starten".into();
        } else {
            d.cause = format!("Test `{test}` schlägt fehl: {reason}");
            d.remedy = "Test und Code prüfen – dafür gibt es keinen automatischen Fix".into();
        }
        return d;
    }

    let last = log
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with('$'));
    d.cause = match last {
        Some(line) => format!("Das Log endet mit: {line}"),
        None => "Das Log ist leer – die Ursache lässt sich nicht eingrenzen.".into(),
    };
    d.remedy = "Check neu starten und das Log beobachten".into();
    d
}

#[cfg(test)]
mod tests;
