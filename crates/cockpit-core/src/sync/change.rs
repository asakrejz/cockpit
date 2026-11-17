//! What changed from outside, as the log shows it.

use super::*;

/// What changed from outside during the sync. The log entries arise from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    ItemAdded {
        id: String,
        key: String,
        title: String,
    },
    ItemRemoved {
        id: String,
        key: String,
    },
    NewComment {
        id: String,
        key: String,
        author: String,
    },
    StatusChanged {
        id: String,
        key: String,
        to: TicketStatus,
    },
    ChecksChanged {
        id: String,
        number: PrKey,
        to: Checks,
    },
    ChangesRequested {
        id: String,
        number: PrKey,
    },
    /// An own, not yet sent comment no longer fits the diff after new commits.
    DraftDropped {
        number: PrKey,
        place: String,
        text: String,
    },
    SourceProblem {
        source: Source,
        problem: Problem,
    },
    SourceRecovered {
        source: Source,
    },
    /// Collective line when many entries come or go at once.
    Summary(String),
}

impl Change {
    /// Short German sentence for the log.
    pub fn text(&self) -> String {
        match self {
            Change::ItemAdded { key, title, .. } => format!("{key} neu: {title}"),
            Change::ItemRemoved { key, .. } => {
                format!("{key} ist nicht mehr dabei (erledigt oder entfernt)")
            }
            Change::NewComment { key, author, .. } => format!("{author} hat {key} kommentiert"),
            Change::StatusChanged { key, to, .. } => {
                format!(
                    "Status von {key} ist jetzt „{}“ (von außen geändert)",
                    label(*to)
                )
            }
            Change::ChecksChanged { number, to, .. } => {
                format!("Checks für {number}: {}", to.label())
            }
            Change::ChangesRequested { number, .. } => {
                format!("Änderungen angefordert in {number}")
            }
            Change::DraftDropped {
                number,
                place,
                text,
            } => format!(
                "Dein Entwurf an {place} in {number} passt nicht mehr zum geänderten Diff und entfällt: „{text}“"
            ),
            Change::SourceProblem { source, problem } => {
                format!("{}: {}", source.label(), problem.text())
            }
            Change::SourceRecovered { source } => format!("{}: wieder verbunden", source.label()),
            Change::Summary(text) => text.clone(),
        }
    }

    pub fn target(&self) -> Option<Target> {
        match self {
            Change::ItemAdded { id, .. }
            | Change::ItemRemoved { id, .. }
            | Change::NewComment { id, .. }
            | Change::StatusChanged { id, .. }
            | Change::ChecksChanged { id, .. }
            | Change::ChangesRequested { id, .. } => Some(Target::Item(id.clone())),
            Change::DraftDropped { number, .. } => Some(Target::Review(number.clone())),
            Change::SourceProblem { .. } | Change::SourceRecovered { .. } | Change::Summary(_) => {
                None
            }
        }
    }
}

pub(super) fn label(status: TicketStatus) -> &'static str {
    TicketStatus::OPTIONS
        .iter()
        .find(|(id, _)| *id == status.id())
        .map_or("", |(_, l)| l)
}
