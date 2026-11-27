//! The inbox lists: which entries a filter shows, what needs attention, and the counts.

use super::*;
use crate::attention::{self, Hit, Rule};

impl Workspace {
    /// Inbox entries for a filter, in display order.
    pub fn visible(&self, filter: Filter) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| filter.matches(i.scope))
            .collect()
    }

    /// Inbox entries by owner, source and search text.
    ///
    /// The rules count: an entry is only in if at least one
    /// switched-on rule applies and it is not snoozed. The filter
    /// "Snoozed" shows the snoozed entries instead.
    pub fn search(
        &self,
        filter: Filter,
        source: SourceFilter,
        query: &str,
        now_ms: u64,
    ) -> Vec<&Item> {
        self.attention(filter, now_ms)
            .into_iter()
            .filter(|i| source.matches(i.source) && crate::search::matches_query(i, query))
            .filter(|i| self.in_project(i))
            .collect()
    }

    /// Inbox entries for the chosen filters: like [`Workspace::search`], then narrowed by the
    /// [`InboxFilter`].
    ///
    /// The filter decides the base list. Every filter but "Snoozed" lists the entries of colleagues
    /// too (pull requests and tickets without a reason for attention) – they never raise
    /// attention or notifications themselves. "Snoozed" keeps the snoozed entries only (see
    /// [`Workspace::search`]). The sprint "Current sprint" falls back to "All open" as long as
    /// no ticket carries sprint data (a project without sprints).
    pub fn inbox(
        &self,
        filter: Filter,
        source: SourceFilter,
        query: &str,
        now_ms: u64,
        by: &InboxFilter,
    ) -> Vec<&Item> {
        let by = self.effective_filter(by);
        let base: Vec<&Item> = if filter == Filter::Snoozed {
            self.search(filter, source, query, now_ms)
        } else {
            self.visible(filter)
                .into_iter()
                .filter(|i| !self.is_snoozed(&i.id, now_ms))
                .filter(|i| source.matches(i.source) && crate::search::matches_query(i, query))
                .filter(|i| self.in_project(i))
                .collect()
        };
        let base = base.into_iter().filter(|i| self.team_visible(i)).collect();
        by.apply(base, now_ms)
    }

    /// Entries that need attention for this filter (or, for
    /// [`Filter::Snoozed`], are currently snoozed), in display order.
    pub fn attention(&self, filter: Filter, now_ms: u64) -> Vec<&Item> {
        self.visible(filter)
            .into_iter()
            .filter(|i| match filter {
                Filter::Snoozed => self.is_snoozed(&i.id, now_ms),
                _ => !self.is_snoozed(&i.id, now_ms) && self.needs_attention(i, now_ms),
            })
            .collect()
    }

    /// Hits of the switched-on rules for an entry, most important first.
    pub fn hits(&self, item: &Item, now_ms: u64) -> Vec<Hit> {
        attention::hits(item, &self.rules, now_ms)
    }

    pub fn needs_attention(&self, item: &Item, now_ms: u64) -> bool {
        !self.hits(item, now_ms).is_empty()
    }

    /// Switches a rule on or off. `false` if nothing changes.
    pub fn set_rule(&mut self, rule: Rule, enabled: bool) -> bool {
        self.rules.set(rule, enabled)
    }

    /// Entries that still need attention. For [`Filter::Snoozed`]: how many are snoozed.
    pub fn unread(&self, filter: Filter, now_ms: u64) -> usize {
        self.attention(filter, now_ms)
            .iter()
            .filter(|i| filter == Filter::Snoozed || !i.read)
            .count()
    }

    /// The badge of the "Tasks" entry: the unread entries that need attention, every entry counted once.
    /// Since "Ich" is a subset of "Alle", this is the unread number of the union of both lists.
    pub fn tasks_unread(&self, now_ms: u64) -> usize {
        let mut ids: Vec<&str> = [Filter::Mine, Filter::All]
            .into_iter()
            .flat_map(|f| self.attention(f, now_ms))
            .filter(|i| !i.read)
            .map(|i| i.id.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    }
}
