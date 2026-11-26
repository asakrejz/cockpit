//! The inbox filters: one list says which filters exist, what they are called, which choices
//! they offer and what they start with. The filter dialog renders this list. The values live in
//! [`InboxFilter`], which reads and writes them by filter ID (`get`/`set`), always as text
//! where the empty text means „all“, and decides which entries match.
//!
//! The chosen values are part of the workspace ([`Workspace::inbox_filter`](crate::Workspace))
//! and change only by [`Action::SetInboxFilter`](crate::Action).
//!
//! Filters whose choices come from the data (repos, people, projects, labels) have
//! [`Choices::FromData`]; [`Workspace::filter_choices`](crate::Workspace::filter_choices) lists
//! them.

mod inbox;

pub use inbox::InboxFilter;

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
    /// The IDs, in display order; the interface words them.
    pub const IDS: &'static [&'static str] = &["current", "open", "recent", "backlog"];

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

/// "Recently updated" means this many days.
pub const RECENT_DAYS: u64 = 14;

/// The value of the project filter for pull requests without a ticket key.
pub const NO_TICKET: &str = "-";

/// The value of the person filter for „assigned to me“.
pub const ASSIGNED_TO_ME: &str = "@me";

/// The value of the person filter for tickets nobody is assigned to.
pub const UNASSIGNED: &str = "@none";

/// The ID the chip of the chosen Jira project carries (it is no inbox filter, but a setting of
/// the workspace).
pub const JIRA_PROJECT_FILTER: &str = "jira_project";

/// A filter that is not at its start value, as shown in the chip under the filter button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveFilter {
    /// Filter ID, or [`JIRA_PROJECT_FILTER`]; the interface words it.
    pub id: &'static str,
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
    /// The IDs, in display order; the interface words them.
    Fixed(&'static [&'static str]),
    /// From the entries in the workspace (see `Workspace::filter_choices`).
    FromData,
}

/// One filter of the inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterDef {
    /// Stable ID: key for `InboxFilter::get`/`set`.
    pub id: &'static str,
    /// The filter has the choice „no restriction“ (its value is the empty text).
    pub has_all: bool,
    pub choices: Choices,
    /// Value at the start (the empty text = „all“).
    pub default: &'static str,
    pub applies: Applies,
}

const DONE_IDS: &[&str] = &["on", "off"];

const WATCHED_IDS: &[&str] = &["off", "on"];

/// All inbox filters, in display order.
pub const FILTERS: &[FilterDef] = &[
    FilterDef {
        id: "source",
        has_all: false,
        choices: Choices::Fixed(crate::SourceFilter::IDS),
        default: "all",
        applies: Applies::Both,
    },
    FilterDef {
        id: "sprint",
        has_all: false,
        choices: Choices::Fixed(SprintView::IDS),
        default: "current",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "project",
        has_all: true,
        choices: Choices::FromData,
        default: "",
        applies: Applies::Both,
    },
    FilterDef {
        id: "repo",
        has_all: true,
        choices: Choices::FromData,
        default: "",
        applies: Applies::PullRequests,
    },
    FilterDef {
        id: "person",
        has_all: true,
        choices: Choices::FromData,
        default: "",
        applies: Applies::Both,
    },
    FilterDef {
        id: "reporter",
        has_all: true,
        choices: Choices::FromData,
        default: "",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "watched",
        has_all: false,
        choices: Choices::Fixed(WATCHED_IDS),
        default: "off",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "status",
        has_all: true,
        choices: Choices::Fixed(crate::TicketStatus::IDS),
        default: "",
        applies: Applies::Both,
    },
    FilterDef {
        id: "type",
        has_all: true,
        choices: Choices::FromData,
        default: "",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "priority",
        has_all: true,
        choices: Choices::Fixed(crate::Priority::IDS),
        default: "",
        applies: Applies::Tickets,
    },
    FilterDef {
        id: "label",
        has_all: true,
        choices: Choices::FromData,
        default: "",
        applies: Applies::Both,
    },
    FilterDef {
        id: "done",
        has_all: false,
        choices: Choices::Fixed(DONE_IDS),
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
