//! The chosen inbox filters of the workspace: the choices of every filter, the list of active
//! filters, the filter as it acts on this workspace and the change itself.

use super::*;
use crate::filters::{
    ASSIGNED_TO_ME, ActiveFilter, Choices, JIRA_PROJECT_FILTER, NO_TICKET, SprintView, UNASSIGNED,
    filter_def,
};

impl Workspace {
    /// The choices of the person filter: me, nobody, then the people of the entries. The first two
    /// carry their value as label.
    pub(super) fn person_choices(&self) -> Vec<(String, String)> {
        let mut out = vec![
            (ASSIGNED_TO_ME.to_string(), ASSIGNED_TO_ME.to_string()),
            (UNASSIGNED.to_string(), UNASSIGNED.to_string()),
        ];
        out.extend(
            self.owners()
                .into_iter()
                .filter(|p| !p.me && !p.nobody)
                .map(|p| (p.name.clone(), p.name)),
        );
        out
    }

    /// The choices of the reporter filter: me, then the reporters of the tickets, sorted.
    pub(super) fn reporter_choices(&self) -> Vec<(String, String)> {
        let mut names: Vec<String> = self
            .items
            .iter()
            .filter_map(|i| i.reporter.clone())
            .filter(|n| !n.is_empty() && n != ME_MARK)
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        std::iter::once((ASSIGNED_TO_ME.to_string(), ASSIGNED_TO_ME.to_string()))
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

    /// Is the entry in the workspace but not in the list the filters leave?
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
    /// order.
    pub fn active_filters(&self) -> Vec<ActiveFilter> {
        let mut out: Vec<ActiveFilter> = self
            .jira_project
            .iter()
            .map(|p| ActiveFilter {
                id: JIRA_PROJECT_FILTER,
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
                value: shown,
            });
        }
        out
    }

    /// The filter as it acts on this workspace: "Current sprint" without any sprint data in
    /// the tickets counts as "All open".
    pub fn effective_filter(&self, by: &InboxFilter) -> InboxFilter {
        let mut by = by.clone();
        if by.sprint == SprintView::Current && !self.has_sprint_data() {
            by.sprint = SprintView::AllOpen;
        }
        by
    }

    /// Does any ticket carry a sprint?
    pub fn has_sprint_data(&self) -> bool {
        self.items
            .iter()
            .any(|i| i.is_ticket() && i.sprint.is_some())
    }

    /// The choices of a filter with [`Choices::FromData`], as `(value, label)` in display order.
    /// For filters with fixed choices (and unknown IDs) the fixed list.
    pub fn filter_choices(&self, id: &str) -> Vec<(String, String)> {
        let same = |v: String| (v.clone(), v);
        match id {
            "repo" => self.item_repos().into_iter().map(same).collect(),
            "person" => self.person_choices(),
            "reporter" => self.reporter_choices(),
            "type" => self.type_choices(),
            "label" => self.labels().into_iter().map(same).collect(),
            "project" => {
                let mut list: Vec<String> = self
                    .items
                    .iter()
                    .filter_map(Item::ticket_project)
                    .chain(self.jira_project.clone())
                    .collect();
                list.sort();
                list.dedup();
                let mut out: Vec<(String, String)> = list.into_iter().map(same).collect();
                if self
                    .items
                    .iter()
                    .any(|i| i.kind == Kind::PullRequest && i.ticket_project().is_none())
                {
                    out.push((NO_TICKET.into(), NO_TICKET.into()));
                }
                out
            }
            other => match crate::filters::filter_def(other).map(|d| d.choices) {
                Some(Choices::Fixed(list)) => list
                    .iter()
                    .map(|v| (v.to_string(), v.to_string()))
                    .collect(),
                _ => Vec::new(),
            },
        }
    }
}
