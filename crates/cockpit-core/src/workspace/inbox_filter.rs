//! The chosen inbox filters of the workspace: choices of the person filter, the list of active
//! filters (for the badge and the chips) and the change itself.

use super::*;
use crate::filters::{
    ASSIGNED_TO_ME, ActiveFilter, JIRA_PROJECT_FILTER, UNASSIGNED, UNASSIGNED_NAME, filter_def,
};

impl Workspace {
    /// The choices of the person filter: me, nobody, then the people of the entries.
    /// „Alle“ is the filter's own „all“ choice.
    pub(super) fn person_choices(&self) -> Vec<(String, String)> {
        let mut out = vec![
            (ASSIGNED_TO_ME.to_string(), "Mir".to_string()),
            (UNASSIGNED.to_string(), "Nicht zugewiesen".to_string()),
        ];
        out.extend(
            self.owners()
                .into_iter()
                .filter(|p| *p != Person::me() && p.name != UNASSIGNED_NAME)
                .map(|p| (p.name.clone(), p.name)),
        );
        out
    }

    /// The choices of the reporter filter: me, then the reporters of the tickets, sorted.
    pub(super) fn reporter_choices(&self) -> Vec<(String, String)> {
        let me = Person::me().name;
        let mut names: Vec<String> = self
            .items
            .iter()
            .filter_map(|i| i.reporter.clone())
            .filter(|n| !n.is_empty() && *n != me)
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        std::iter::once((ASSIGNED_TO_ME.to_string(), "Mir".to_string()))
            .chain(names.into_iter().map(|n| (n.clone(), n)))
            .collect()
    }

    /// The issue types of the tickets, alphabetical and without duplicates.
    pub(super) fn type_choices(&self) -> Vec<(String, String)> {
        let mut types: Vec<String> = Vec::new();
        for t in self.items.iter().filter_map(|i| i.issue_type.as_ref()) {
            if !t.is_empty() && !types.iter().any(|o| o.eq_ignore_ascii_case(t)) {
                types.push(t.clone());
            }
        }
        types.sort_by_key(|t| t.to_lowercase());
        types.into_iter().map(|t| (t.clone(), t)).collect()
    }

    /// Is the entry in the workspace but not in the list the filters leave? The inbox shows such
    /// an entry anyway when it is opened by address, and says so.
    pub fn hidden_by_filter(
        &self,
        id: &str,
        filter: Filter,
        query: &str,
        now_ms: u64,
        by: &InboxFilter,
    ) -> bool {
        self.item(id).is_some()
            && !self
                .inbox(filter, SourceFilter::All, query, now_ms, by)
                .iter()
                .any(|i| i.id == id)
    }

    /// The filters that show the entry again: the start values if they do, otherwise the start
    /// values with every sprint and done tickets as well (an entry from the backlog or a done
    /// one is hidden by the start values themselves).
    pub fn filters_revealing(&self, id: &str, filter: Filter, now_ms: u64) -> InboxFilter {
        let start = InboxFilter::default();
        if !self.hidden_by_filter(id, filter, "", now_ms, &start) {
            return start;
        }
        InboxFilter {
            sprint: SprintView::AllOpen,
            hide_done: false,
            ..start
        }
    }

    /// Replaces the chosen inbox filters. `false` if nothing changes.
    pub fn set_inbox_filter(&mut self, filter: InboxFilter) -> bool {
        if self.inbox_filter == filter {
            return false;
        }
        self.inbox_filter = filter;
        true
    }

    /// The filters that are not at their start value, plus the chosen Jira project, in display
    /// order. Their number is the badge of the filter button.
    pub fn active_filters(&self) -> Vec<ActiveFilter> {
        let mut out: Vec<ActiveFilter> = self
            .jira_project
            .iter()
            .map(|p| ActiveFilter {
                id: JIRA_PROJECT_FILTER,
                label: "Jira-Projekt",
                value: p.clone(),
            })
            .collect();
        for id in self.inbox_filter.active() {
            let Some(def) = filter_def(id) else { continue };
            let value = self.inbox_filter.get(id);
            let shown = self
                .filter_choices(id)
                .into_iter()
                .find(|(v, _)| *v == value)
                .map_or(value, |(_, label)| label);
            out.push(ActiveFilter {
                id: def.id,
                label: def.label,
                value: shown,
            });
        }
        out
    }
}
