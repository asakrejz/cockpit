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
    pub(super) fn new(target: Target, text: impl Into<String>) -> Self {
        Self {
            actor: None,
            target: Some(target),
            text: text.into(),
            msg: None,
        }
    }

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

    /// An entry without a specific target (settings).
    pub(super) fn general(text: impl Into<String>) -> Self {
        Self {
            actor: None,
            target: None,
            text: text.into(),
            msg: None,
        }
    }

    pub(super) fn by(mut self, actor: Actor) -> Self {
        self.actor = Some(actor);
        self
    }
}

pub(super) fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

pub(super) fn status_label(status: TicketStatus) -> &'static str {
    TicketStatus::OPTIONS
        .iter()
        .find(|(id, _)| *id == status.id())
        .map_or("", |(_, label)| label)
}

/// Log entries of the system for status changes that the pull request triggered.
pub(super) fn status_notes(changes: Vec<StatusChange>) -> Vec<Note> {
    changes
        .into_iter()
        .map(|c| {
            let why = match c.to {
                TicketStatus::Done => format!("{} ist gemergt", c.pr),
                TicketStatus::Review => format!("für {} ist ein Review angefragt", c.pr),
                _ => format!("{} ist noch in Arbeit", c.pr),
            };
            Note::new(
                Target::Item(c.item),
                format!(
                    "Status von {} automatisch auf „{}“ gesetzt – {why}",
                    c.key,
                    status_label(c.to)
                ),
            )
            .by(Actor::System)
        })
        .collect()
}
