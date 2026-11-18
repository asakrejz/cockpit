use cockpit_i18n::msg;

use super::*;

/// An entry that an action wants to write into the log.
pub(super) struct Note {
    pub(super) actor: Option<Actor>,
    pub(super) target: Option<Target>,
    pub(super) text: String,
    /// The same sentence as code and parameters; the UI renders it in its language.
    pub(super) msg: Option<cockpit_i18n::Msg>,
}

impl Note {
    /// An entry as code and parameters (`msg!`). `text` keeps the German rendering next to it,
    /// for readers that only know plain text.
    pub(super) fn coded(target: Target, msg: cockpit_i18n::Msg) -> Self {
        Self {
            actor: None,
            target: Some(target),
            text: msg.de(),
            msg: Some(msg),
        }
    }

    /// Like [`Note::coded`], without a target (settings).
    pub(super) fn coded_general(msg: cockpit_i18n::Msg) -> Self {
        Self {
            actor: None,
            target: None,
            text: msg.de(),
            msg: Some(msg),
        }
    }

    pub(super) fn by(mut self, actor: Actor) -> Self {
        self.actor = Some(actor);
        self
    }
}

/// Log entries of the system for status changes that the pull request triggered.
pub(super) fn status_notes(changes: Vec<StatusChange>) -> Vec<Note> {
    changes
        .into_iter()
        .map(|c| {
            let pr = c.pr.to_string();
            let why = match c.to {
                TicketStatus::Done => msg!(CoreActionStatusWhyMerged, pr = pr),
                TicketStatus::Review => msg!(CoreActionStatusWhyReview, pr = pr),
                _ => msg!(CoreActionStatusWhyProgress, pr = pr),
            };
            Note::coded(
                Target::Item(c.item),
                msg!(
                    CoreActionLogStatusAuto,
                    key = c.key,
                    status = labels::status(c.to),
                    why = why
                ),
            )
            .by(Actor::System)
        })
        .collect()
}
