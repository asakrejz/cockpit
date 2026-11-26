//! "Viewed": which files of a pull request the person has looked at. A file counts as viewed
//! only as long as its diff is the one that was looked at – a new push makes it open again.
//! The browser keeps this per pull request.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{FileDiff, LineKind};

impl FileDiff {
    /// Identifies the content of the diff (FNV-1a over name, kinds, numbers and texts). Stable
    /// across runs and platforms, unlike `std`'s hashers.
    pub fn fingerprint(&self) -> u64 {
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |bytes: &[u8]| {
            for b in bytes {
                hash ^= u64::from(*b);
                hash = hash.wrapping_mul(PRIME);
            }
        };
        feed(self.name.as_bytes());
        for line in &self.lines {
            feed(&[match line.kind {
                LineKind::Context => 0,
                LineKind::Add => 1,
                LineKind::Del => 2,
            }]);
            feed(&line.no.to_le_bytes());
            feed(line.text.as_bytes());
            feed(&[0xff]);
        }
        hash
    }
}

/// The viewed files of one pull request: name → fingerprint at the time of viewing.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Viewed(BTreeMap<String, u64>);

impl Viewed {
    /// Reads what the browser stored. Anything unreadable counts as „nothing viewed“.
    pub fn parse(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }

    pub fn is_viewed(&self, file: &FileDiff) -> bool {
        self.0.get(&file.name) == Some(&file.fingerprint())
    }

    pub fn set(&mut self, file: &FileDiff, viewed: bool) {
        if viewed {
            self.0.insert(file.name.clone(), file.fingerprint());
        } else {
            self.0.remove(&file.name);
        }
    }

    /// How many of `files` are viewed.
    pub fn count(&self, files: &[FileDiff]) -> usize {
        files.iter().filter(|f| self.is_viewed(f)).count()
    }
}

#[cfg(test)]
mod tests;
