//! The inbox filters as data: one list says which filters exist, what they are called, which
//! choices they offer and what they start with. The filter dialog renders this list. The values
//! live in [`InboxFilter`](crate::InboxFilter),
//! which reads and writes them by filter ID (`get`/`set`), always as text where the empty text
//! means „all“.
//!
//! The chosen values are part of the workspace ([`Workspace::inbox_filter`](crate::Workspace))
//! and change only by [`Action::SetInboxFilter`](crate::Action).
//!
//! Filters whose choices come from the data (repos, people, projects, labels) have
//! [`Choices::FromData`]; [`Workspace::filter_choices`](crate::Workspace::filter_choices) lists
//! them.

/// Whose entries are listed. „Alle“ (the start value) lists the entries of colleagues too,
/// without attention; „Meine“ keeps only what needs your attention.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Ownership {
    Mine,
    #[default]
    All,
}

impl Ownership {
    pub const OPTIONS: &'static [(&'static str, &'static str)] =
        &[("mine", "Meine"), ("all", "Alle")];

    pub fn id(self) -> &'static str {
        match self {
            Ownership::Mine => "mine",
            Ownership::All => "all",
        }
    }

    pub fn from_id(id: &str) -> Option<Ownership> {
        match id {
            "mine" => Some(Ownership::Mine),
            "all" => Some(Ownership::All),
            _ => None,
        }
    }
}

/// Which tickets count by their sprint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SprintView {
    /// Tickets of the active sprint.
    #[default]
    Current,
    /// No restriction by sprint.
    AllOpen,
    /// Tickets updated in the last [`RECENT_DAYS`] days.
    Recent,
    /// Tickets without a sprint.
    Backlog,
}

impl SprintView {
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("current", "Aktueller Sprint"),
        ("open", "Alle offenen"),
        ("recent", "Zuletzt aktualisiert (14 Tage)"),
        ("backlog", "Backlog (kein Sprint)"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            SprintView::Current => "current",
            SprintView::AllOpen => "open",
            SprintView::Recent => "recent",
            SprintView::Backlog => "backlog",
        }
    }

    pub fn from_id(id: &str) -> Option<SprintView> {
        [
            SprintView::Current,
            SprintView::AllOpen,
            SprintView::Recent,
            SprintView::Backlog,
        ]
        .into_iter()
        .find(|v| v.id() == id)
    }
}

/// „Zuletzt aktualisiert“ means this many days.
pub const RECENT_DAYS: u64 = 14;

/// The value of the project filter for pull requests without a ticket key.
pub const NO_TICKET: &str = "-";

/// The value of the person filter for „assigned to me“.
pub const ASSIGNED_TO_ME: &str = "@me";

/// The value of the person filter for tickets nobody is assigned to.
pub const UNASSIGNED: &str = "@none";

/// The name Jira tickets without an assignee carry as owner.
pub const UNASSIGNED_NAME: &str = "Nicht zugewiesen";

/// The ID the chip of the chosen Jira project carries (it is no inbox filter, but a setting of
/// the workspace).
pub const JIRA_PROJECT_FILTER: &str = "jira_project";

/// A filter that is not at its start value, as shown in the chip under the filter button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveFilter {
    /// Filter ID, or [`JIRA_PROJECT_FILTER`].
    pub id: &'static str,
    /// Name of the filter (German).
    pub label: &'static str,
    /// The chosen value as the person reads it.
    pub value: String,
}

/// Which entries a filter acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applies {
    Both,
    Tickets,
    PullRequests,
}

/// Where the choices of a filter come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choices {
    /// ID and label, in display order.
    Fixed(&'static [(&'static str, &'static str)]),
    /// From the entries in the workspace (see `Workspace::filter_choices`).
    FromData,
}

/// One filter of the inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterDef {
    /// Stable ID: key for `InboxFilter::get`/`set`.
    pub id: &'static str,
    /// Name for the label of the control and the dialog (German).
    pub label: &'static str,
    /// Label of the choice „no restriction“ (value is the empty text). `None`: the filter
    /// has no such choice.
    pub all_label: Option<&'static str>,
    pub choices: Choices,
    /// Value at the start (the empty text = „all“).
    pub default: &'static str,
    pub applies: Applies,
}

const DONE_OPTIONS: &[(&str, &str)] =
    &[("on", "Erledigte ausblenden"), ("off", "Erledigte zeigen")];

const WATCHED_OPTIONS: &[(&str, &str)] = &[("off", "Egal"), ("on", "Nur beobachtete")];

/// All inbox filters, in display order.
pub const FILTERS: &[FilterDef] = &[
    FilterDef {
        id: "source",
        label: "Quelle",
        all_label: None,
        choices: Choices::Fixed(crate::SourceFilter::OPTIONS),
        default: "all",
        applies: Applies::Both,
    },
    FilterDef {
        id: "ownership",
        label: "Besitz",
        all_label: None,
        choices: Choices::Fixed(Ownership::OPTIONS),
        default: "all",
        applies: Applies::Both,
    },
    FilterDef {
        id: "sprint",
        label: "Sprint",
        all_label: None,
        choices: Choices::Fixed(SprintView::OPTIONS),
        default: "current",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "project",
        label: "Ticket-Projekt",
        all_label: Some("Alle Ticket-Projekte"),
        choices: Choices::FromData,
        default: "",
        applies: Applies::Both,
    },
    FilterDef {
        id: "repo",
        label: "Repo",
        all_label: Some("Alle Repos"),
        choices: Choices::FromData,
        default: "",
        applies: Applies::PullRequests,
    },
    FilterDef {
        id: "person",
        label: "Bearbeiter",
        all_label: Some("Alle"),
        choices: Choices::FromData,
        default: "",
        applies: Applies::Both,
    },
    FilterDef {
        id: "reporter",
        label: "Ersteller",
        all_label: Some("Alle"),
        choices: Choices::FromData,
        default: "",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "watched",
        label: "Beobachtet von mir",
        all_label: None,
        choices: Choices::Fixed(WATCHED_OPTIONS),
        default: "off",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "status",
        label: "Status",
        all_label: Some("Alle Status"),
        choices: Choices::Fixed(crate::TicketStatus::OPTIONS),
        default: "",
        applies: Applies::Both,
    },
    FilterDef {
        id: "type",
        label: "Typ",
        all_label: Some("Alle Typen"),
        choices: Choices::FromData,
        default: "",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "priority",
        label: "Priorität",
        all_label: Some("Alle Prioritäten"),
        choices: Choices::Fixed(crate::Priority::OPTIONS),
        default: "",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "label",
        label: "Label",
        all_label: Some("Alle Labels"),
        choices: Choices::FromData,
        default: "",
        applies: Applies::Both,
    },
    FilterDef {
        id: "done",
        label: "Erledigte",
        all_label: None,
        choices: Choices::Fixed(DONE_OPTIONS),
        default: "on",
        applies: Applies::Tickets,
    },
];

/// The definition of a filter by ID.
pub fn filter_def(id: &str) -> Option<&'static FilterDef> {
    FILTERS.iter().find(|f| f.id == id)
}

#[cfg(test)]
mod tests;
