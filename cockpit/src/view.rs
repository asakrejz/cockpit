//! Übersetzt Begriffe der Fachlogik in Bausteine der Oberfläche.

use cockpit_core::{FileDiff, LineKind, ReasonKind, Tone};
use feature_ui::{DiffKind, DiffRow, StatusTone, icons};

pub fn tone(tone: Tone) -> StatusTone {
    match tone {
        Tone::Gray => StatusTone::Gray,
        Tone::Blue => StatusTone::Blue,
        Tone::Green => StatusTone::Green,
        Tone::Orange => StatusTone::Orange,
        Tone::Red => StatusTone::Red,
        Tone::Indigo => StatusTone::Indigo,
    }
}

pub fn reason_icon(reason: ReasonKind) -> &'static str {
    match reason {
        ReasonKind::Comment => icons::COMMENT,
        ReasonKind::ReviewRequested => icons::EYE,
        ReasonKind::AgentFinished => icons::SPARKLE,
        ReasonKind::ChecksFailed => icons::ALERT,
        ReasonKind::Assigned => icons::ASSIGN,
    }
}

pub fn diff_rows(file: &FileDiff) -> Vec<DiffRow> {
    file.lines
        .iter()
        .map(|line| DiffRow {
            no: line.no,
            kind: match line.kind {
                LineKind::Context => DiffKind::Context,
                LineKind::Add => DiffKind::Add,
                LineKind::Del => DiffKind::Del,
            },
            text: line.text.clone(),
        })
        .collect()
}
