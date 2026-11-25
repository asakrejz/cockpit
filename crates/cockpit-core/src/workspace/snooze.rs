//! Snoozing: putting an entry away until a time or until something changes.

use super::*;
use crate::attention::Snooze;

impl Workspace {
    pub fn snooze_of(&self, id: &str) -> Option<&Snooze> {
        self.snoozed.iter().find(|s| s.item == id)
    }

    /// Is the entry currently snoozed?
    pub fn is_snoozed(&self, id: &str, now_ms: u64) -> bool {
        match (self.snooze_of(id), self.item(id)) {
            (Some(snooze), Some(item)) => snooze.holds(item, now_ms),
            _ => false,
        }
    }

    /// IDs of the entries whose snooze has expired or been lifted by a change
    /// (and which `wake` therefore still has to remove).
    pub fn expired_snoozes(&self, now_ms: u64) -> Vec<String> {
        self.snoozed
            .iter()
            .filter(|s| !self.is_snoozed(&s.item, now_ms))
            .map(|s| s.item.clone())
            .collect()
    }

    /// Snoozes an entry (`until_ms`: until then; `None`: until something changes).
    /// An earlier snooze is replaced.
    pub fn snooze(&mut self, id: &str, until_ms: Option<u64>) -> bool {
        let Some(item) = self.item(id) else {
            return false;
        };
        let snooze = Snooze::new(item, until_ms);
        self.snoozed.retain(|s| s.item != id);
        self.snoozed.push(snooze);
        true
    }

    /// Brings an entry back. `false` if it was not snoozed.
    pub fn unsnooze(&mut self, id: &str) -> bool {
        let before = self.snoozed.len();
        self.snoozed.retain(|s| s.item != id);
        self.snoozed.len() != before
    }

    /// Removes expired snoozes. Returns the IDs of the entries that came back.
    pub fn wake(&mut self, now_ms: u64) -> Vec<String> {
        let back = self.expired_snoozes(now_ms);
        self.snoozed.retain(|s| !back.contains(&s.item));
        back
    }
}
