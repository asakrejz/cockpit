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
