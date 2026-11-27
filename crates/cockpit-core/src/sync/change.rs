//! What changed from outside.

use cockpit_i18n::{Msg, msg};

use super::*;

/// What changed from outside during the sync.
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
    Summary(Msg),
}

impl Change {
    /// The sentence for the log as code and parameters.
    pub fn msg(&self) -> Msg {
        match self {
            Change::ItemAdded { key, title, .. } => {
                msg!(CoreSyncItemAdded, key = key, title = title)
            }
            Change::ItemRemoved { key, .. } => msg!(CoreSyncItemRemoved, key = key),
            Change::NewComment { key, author, .. } => {
                msg!(CoreSyncNewComment, author = author, key = key)
            }
            Change::StatusChanged { key, to, .. } => msg!(
                CoreSyncStatusChanged,
                key = key,
                status = crate::action::labels::status(*to)
            ),
            Change::ChecksChanged { number, to, .. } => msg!(
                CoreSyncChecksChanged,
                number = number.to_string(),
                state = checks_msg(*to)
            ),
            Change::ChangesRequested { number, .. } => {
                msg!(CoreSyncChangesRequested, number = number.to_string())
            }
            Change::DraftDropped {
                number,
                place,
                text,
            } => msg!(
                CoreSyncDraftDropped,
                place = place,
                number = number.to_string(),
                text = text
            ),
            Change::SourceProblem { source, problem } => msg!(
                CoreSyncSourceProblem,
                source = Msg::raw(source.label()),
                problem = problem.msg()
            ),
            Change::SourceRecovered { source } => {
                msg!(CoreSyncSourceRecovered, source = Msg::raw(source.label()))
            }
            Change::Summary(msg) => msg.clone(),
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

fn checks_msg(checks: Checks) -> Msg {
    match checks {
        Checks::Passed => msg!(CoreSyncChecksPassed),
        Checks::Failed => msg!(CoreSyncChecksFailed),
        Checks::Running => msg!(CoreSyncChecksRunning),
        Checks::Unknown => msg!(CoreSyncChecksUnknown),
    }
}
