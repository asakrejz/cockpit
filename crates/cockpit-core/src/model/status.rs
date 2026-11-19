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
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("offen", "Offen"),
        ("arbeit", "In Arbeit"),
        ("review", "Review"),
        ("fertig", "Fertig"),
    ];

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

/// How important a ticket is. Old states without a value count as „Mittel“.
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

    /// ID and label, in display order (most important first).
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("hoechste", "Höchste"),
        ("hoch", "Hoch"),
        ("mittel", "Mittel"),
        ("niedrig", "Niedrig"),
        ("niedrigste", "Niedrigste"),
    ];

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

    pub fn label(self) -> &'static str {
        Priority::OPTIONS
            .iter()
            .find(|(id, _)| *id == self.id())
            .map_or("", |(_, label)| label)
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
    pub fn label(self) -> &'static str {
        match self {
            Checks::Passed => "Checks bestanden",
            Checks::Failed => "Checks fehlgeschlagen",
            Checks::Running => "Checks laufen",
            Checks::Unknown => "Checks nicht sichtbar",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Checks::Passed => Tone::Green,
            Checks::Failed => Tone::Red,
            Checks::Running => Tone::Blue,
            Checks::Unknown => Tone::Gray,
        }
    }
}
