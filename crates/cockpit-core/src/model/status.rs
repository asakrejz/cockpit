//! Ticket status, priority, tone, the reason of an entry and the checks.

use super::*;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TicketStatus {
    #[default]
    Open,
    InProgress,
    Review,
    Done,
}

impl TicketStatus {
    /// The IDs, in display order; the interface words them.
    pub const IDS: &'static [&'static str] = &["offen", "arbeit", "review", "fertig"];

    /// The status as a message, for log lines and errors.
    pub fn name_msg(self) -> cockpit_i18n::Msg {
        match self {
            TicketStatus::Open => cockpit_i18n::msg!(CoreActionStatusOpen),
            TicketStatus::InProgress => cockpit_i18n::msg!(CoreActionStatusProgress),
            TicketStatus::Review => cockpit_i18n::msg!(CoreActionStatusReview),
            TicketStatus::Done => cockpit_i18n::msg!(CoreActionStatusDone),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            TicketStatus::Open => "offen",
            TicketStatus::InProgress => "arbeit",
            TicketStatus::Review => "review",
            TicketStatus::Done => "fertig",
        }
    }

    pub fn from_id(id: &str) -> Option<TicketStatus> {
        [
            TicketStatus::Open,
            TicketStatus::InProgress,
            TicketStatus::Review,
            TicketStatus::Done,
        ]
        .into_iter()
        .find(|s| s.id() == id)
    }
}

/// How important a ticket is. Old states without a value count as medium.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Priority {
    Highest,
    High,
    #[default]
    Medium,
    Low,
    Lowest,
}

impl Priority {
    pub const ALL: [Priority; 5] = [
        Priority::Highest,
        Priority::High,
        Priority::Medium,
        Priority::Low,
        Priority::Lowest,
    ];

    /// The IDs, in display order (most important first); the interface words them.
    pub const IDS: &'static [&'static str] =
        &["hoechste", "hoch", "mittel", "niedrig", "niedrigste"];

    pub fn id(self) -> &'static str {
        match self {
            Priority::Highest => "hoechste",
            Priority::High => "hoch",
            Priority::Medium => "mittel",
            Priority::Low => "niedrig",
            Priority::Lowest => "niedrigste",
        }
    }

    pub fn from_id(id: &str) -> Option<Priority> {
        Priority::ALL.into_iter().find(|p| p.id() == id)
    }
}

/// Hue for status dots and icons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Gray,
    Blue,
    Green,
    Orange,
    Red,
    Indigo,
}

/// Why an entry needs attention.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasonKind {
    Comment,
    /// Someone mentioned me in a comment with `@Name`.
    Mention,
    ReviewRequested,
    AgentFinished,
    ChecksFailed,
    /// Changes were requested for my pull request.
    ChangesRequested,
    /// The due date of a ticket lies in the past.
    Overdue,
    Assigned,
}

impl ReasonKind {
    pub fn tone(self) -> Tone {
        match self {
            ReasonKind::Comment => Tone::Blue,
            ReasonKind::Mention => Tone::Blue,
            ReasonKind::ReviewRequested => Tone::Orange,
            ReasonKind::AgentFinished => Tone::Green,
            ReasonKind::ChecksFailed | ReasonKind::ChangesRequested => Tone::Red,
            ReasonKind::Overdue => Tone::Orange,
            ReasonKind::Assigned => Tone::Indigo,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checks {
    Passed,
    Failed,
    Running,
    /// The token cannot read checks (a fine-grained token has no "Checks" permission) and the
    /// commit status says nothing either. The pull request itself is unaffected.
    Unknown,
}

impl Checks {
    pub fn tone(self) -> Tone {
        match self {
            Checks::Passed => Tone::Green,
            Checks::Failed => Tone::Red,
            Checks::Running => Tone::Blue,
            Checks::Unknown => Tone::Gray,
        }
    }
}
