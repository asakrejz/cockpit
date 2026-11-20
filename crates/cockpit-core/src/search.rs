//! Search and filter of the inbox – pure functions, without state.

use crate::filters::{
    ASSIGNED_TO_ME, FILTERS, NO_TICKET, Ownership, RECENT_DAYS, SprintView, UNASSIGNED,
    UNASSIGNED_NAME,
};
use crate::model::{Filter, Item, Kind, Person, Priority, SourceFilter, SprintState, TicketStatus};
use serde::{Deserialize, Serialize};

/// Does the entry match the search text?
///
/// Searches title, key (`BAU-142`, `#87`), reason and owner, without regard to
/// case. The text is split into words at spaces;
/// every word must occur somewhere. An empty search text matches everything.
///
/// ```
/// use cockpit_core::{demo, search::matches_query};
/// let ws = demo::workspace();
/// let item = ws.item_by_key("BAU-142").unwrap();
/// assert!(matches_query(item, "bau-142"));
/// assert!(!matches_query(item, "gibt-es-nicht"));
/// ```
pub fn matches_query(item: &Item, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let haystack = [
        item.title.as_str(),
        item.key.as_str(),
        item.reason_text.as_str(),
        item.owner.name.as_str(),
    ]
    .join("\n")
    .to_lowercase();
    query.split_whitespace().all(|word| haystack.contains(word))
}

/// The values of the inbox filters (definitions: [`crate::filters::FILTERS`]). The Jira
/// project of the workspace ([`crate::Workspace::in_project`]) is a setting of its own and not
/// part of it. The value is stored in the workspace (`Workspace::inbox_filter`) and changes by
/// `Action::SetInboxFilter`; `Default` is the start state (own entries, active sprint, done
/// tickets hidden, everything else „all“).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InboxFilter {
    /// Only Jira or only GitHub.
    pub source: SourceFilter,
    /// `owner/name`; entries without a repo (tickets) drop out as soon as a repo is chosen.
    pub repo: Option<String>,
    /// Name of the owner, [`ASSIGNED_TO_ME`] (me) or [`UNASSIGNED`] (nobody). A display filter
    /// only: nothing is written to Jira.
    pub person: Option<String>,
    /// See [`Item::effective_status`].
    pub status: Option<TicketStatus>,
    /// Without regard to case.
    pub label: Option<String>,
    /// Whose entries are listed (decided by `Workspace::inbox`, not by [`InboxFilter::matches`]).
    /// Stored under its own name, so that the old start value „Meine“ that older states carry
    /// is dropped and the new start value „Alle“ applies.
    #[serde(rename = "whose")]
    pub ownership: Ownership,
    /// Name of the reporter of a ticket, [`ASSIGNED_TO_ME`] (me).
    pub reporter: Option<String>,
    /// Only tickets I watch.
    pub watched: bool,
    /// The issue type of a ticket (`Bug`, `Story` …), without regard to case.
    pub issue_type: Option<String>,
    /// The priority of a ticket.
    pub priority: Option<Priority>,
    /// Jira project of the ticket, or of the ticket key in branch and title of a pull request;
    /// [`NO_TICKET`] = pull requests without a ticket key.
    pub project: Option<String>,
    /// Which tickets count by their sprint. Pull requests are not affected.
    pub sprint: SprintView,
    /// Hide done tickets.
    pub hide_done: bool,
}

impl Default for InboxFilter {
    fn default() -> Self {
        Self {
            source: SourceFilter::default(),
            repo: None,
            person: None,
            status: None,
            label: None,
            ownership: Ownership::default(),
            reporter: None,
            watched: false,
            issue_type: None,
            priority: None,
            project: None,
            sprint: SprintView::default(),
            hide_done: true,
        }
    }
}

impl InboxFilter {
    /// Is the filter in its start state?
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The value of a filter by ID as text (`""` = all). Unknown ID: `""`.
    pub fn get(&self, id: &str) -> String {
        match id {
            "source" => self.source.id().into(),
            "ownership" => self.ownership.id().into(),
            "sprint" => self.sprint.id().into(),
            "done" => if self.hide_done { "on" } else { "off" }.into(),
            "project" => self.project.clone().unwrap_or_default(),
            "repo" => self.repo.clone().unwrap_or_default(),
            "person" => self.person.clone().unwrap_or_default(),
            "status" => self.status.map(|s| s.id().to_string()).unwrap_or_default(),
            "label" => self.label.clone().unwrap_or_default(),
            "reporter" => self.reporter.clone().unwrap_or_default(),
            "watched" => if self.watched { "on" } else { "off" }.into(),
            "type" => self.issue_type.clone().unwrap_or_default(),
            "priority" => self
                .priority
                .map(|p| p.id().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Sets a filter by ID from text (`""` = all, or the start value for filters without
    /// „all“). `false` for an unknown ID or a value the filter does not know; nothing changes then.
    pub fn set(&mut self, id: &str, value: &str) -> bool {
        let some = |v: &str| (!v.is_empty()).then(|| v.to_string());
        match id {
            "source" => match SourceFilter::from_id(value) {
                Some(v) => self.source = v,
                None => return false,
            },
            "ownership" => match Ownership::from_id(value) {
                Some(v) => self.ownership = v,
                None => return false,
            },
            "sprint" => match SprintView::from_id(value) {
                Some(v) => self.sprint = v,
                None => return false,
            },
            "done" => match value {
                "on" => self.hide_done = true,
                "off" => self.hide_done = false,
                _ => return false,
            },
            "project" => self.project = some(value),
            "repo" => self.repo = some(value),
            "person" => self.person = some(value),
            "status" => match value {
                "" => self.status = None,
                v => match TicketStatus::from_id(v) {
                    Some(s) => self.status = Some(s),
                    None => return false,
                },
            },
            "label" => self.label = some(value),
            "reporter" => self.reporter = some(value),
            "watched" => match value {
                "on" => self.watched = true,
                "off" | "" => self.watched = false,
                _ => return false,
            },
            "type" => self.issue_type = some(value),
            "priority" => match value {
                "" => self.priority = None,
                v => match Priority::from_id(v) {
                    Some(p) => self.priority = Some(p),
                    None => return false,
                },
            },
            _ => return false,
        }
        true
    }

    /// Does the entry match the chosen values (everything except the ownership)? `now_ms` is
    /// the time of day for „zuletzt aktualisiert“.
    ///
    /// ```
    /// use cockpit_core::{InboxFilter, SprintView, demo};
    /// let ws = demo::workspace();
    /// let filter = InboxFilter {
    ///     label: Some("docs".into()),
    ///     sprint: SprintView::AllOpen,
    ///     ..Default::default()
    /// };
    /// assert!(filter.matches(ws.item_by_key("DOC-7").unwrap(), 0));
    /// assert!(!filter.matches(ws.item_by_key("BAU-142").unwrap(), 0));
    /// ```
    pub fn matches(&self, item: &Item, now_ms: u64) -> bool {
        self.source.matches(item.source)
            && self
                .repo
                .as_ref()
                .is_none_or(|r| item.repo.as_ref() == Some(r))
            && self.matches_person(item)
            && self.status.is_none_or(|s| item.effective_status() == s)
            && self.label.as_ref().is_none_or(|l| {
                item.labels
                    .iter()
                    .any(|own| own.to_lowercase() == l.to_lowercase())
            })
            && self.matches_project(item)
            && self.matches_ticket_fields(item)
            && self.matches_ticket_state(item, now_ms)
    }

    fn matches_person(&self, item: &Item) -> bool {
        match self.person.as_deref() {
            None => true,
            Some(ASSIGNED_TO_ME) => item.owner == Person::me(),
            Some(UNASSIGNED) => item.is_ticket() && item.owner.name == UNASSIGNED_NAME,
            Some(name) => item.owner.name == name,
        }
    }

    /// Reporter, watching, type and priority only concern tickets: as soon as one of them is
    /// chosen, pull requests drop out.
    fn matches_ticket_fields(&self, item: &Item) -> bool {
        let chosen = self.reporter.is_some()
            || self.watched
            || self.issue_type.is_some()
            || self.priority.is_some();
        if !chosen {
            return true;
        }
        item.is_ticket()
            && match self.reporter.as_deref() {
                None => true,
                Some(ASSIGNED_TO_ME) => {
                    item.reporter.as_deref() == Some(Person::me().name.as_str())
                }
                Some(name) => item.reporter.as_deref() == Some(name),
            }
            && (!self.watched || item.watching)
            && self.issue_type.as_ref().is_none_or(|t| {
                item.issue_type
                    .as_ref()
                    .is_some_and(|own| own.to_lowercase() == t.to_lowercase())
            })
            && self.priority.is_none_or(|p| item.priority == p)
    }

    fn matches_project(&self, item: &Item) -> bool {
        match self.project.as_deref() {
            None => true,
            Some(NO_TICKET) => item.kind == Kind::PullRequest && item.ticket_project().is_none(),
            Some(p) => item.ticket_project().as_deref() == Some(p),
        }
    }

    /// Sprint and „done“ only concern tickets.
    fn matches_ticket_state(&self, item: &Item, now_ms: u64) -> bool {
        if !item.is_ticket() {
            return true;
        }
        if self.hide_done && item.status == TicketStatus::Done {
            return false;
        }
        match self.sprint {
            SprintView::AllOpen => true,
            // A project ticket without an iteration may come from a project that has none: it
            // stays visible, only a ticket in a past or future iteration drops out.
            SprintView::Current => item.sprint.as_ref().map_or(
                item.source == crate::model::Source::GitHubProject,
                |s| s.state == SprintState::Active,
            ),
            SprintView::Backlog => item.sprint.is_none(),
            SprintView::Recent => item
                .updated_ms
                .is_some_and(|t| t.saturating_add(RECENT_DAYS * 24 * 3600 * 1000) >= now_ms),
        }
    }

    /// Puts one filter back to its start value. `false` for an unknown ID.
    pub fn reset(&mut self, id: &str) -> bool {
        match crate::filters::filter_def(id) {
            Some(def) => self.set(id, def.default),
            None => false,
        }
    }

    /// The IDs of the filters that differ from their start value, in display order.
    pub fn active(&self) -> Vec<&'static str> {
        let start = Self::default();
        FILTERS
            .iter()
            .filter(|def| self.get(def.id) != start.get(def.id))
            .map(|def| def.id)
            .collect()
    }

    /// Keeps the entries that match; the order is preserved.
    pub fn apply<'a>(&self, items: Vec<&'a Item>, now_ms: u64) -> Vec<&'a Item> {
        items
            .into_iter()
            .filter(|i| self.matches(i, now_ms))
            .collect()
    }
}

/// Filters entries by owner, source and search text; the order is preserved.
pub fn search<'a>(
    items: &'a [Item],
    filter: Filter,
    source: SourceFilter,
    query: &str,
) -> Vec<&'a Item> {
    items
        .iter()
        .filter(|i| filter.matches(i.scope) && source.matches(i.source))
        .filter(|i| matches_query(i, query))
        .collect()
}

#[cfg(test)]
mod tests;
