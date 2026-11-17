//! What the services deliver.

use super::*;

/// The state of both services after a fetch. A source that is missing was not queried
/// (not configured, no key, pause because of quota) – its data stays.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Remote {
    #[serde(default)]
    pub sources: Vec<SourceData>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SourceData {
    pub source: Source,
    pub outcome: Outcome,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The complete state of the source. What is missing here has disappeared there.
    Loaded(Loaded),
    /// The fetch failed.
    Failed(Problem),
}

/// The complete state of a source.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    #[serde(default)]
    pub items: Vec<Item>,
    /// GitHub only: the pull requests as a review (diff, checks).
    #[serde(default)]
    pub reviews: Vec<Review>,
    /// GitHub only: which number belongs to which repo.
    #[serde(default)]
    pub prs: Vec<PrLink>,
    /// Hints for the state of the source (e.g. skipped pull requests).
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Which repo a pull request belongs to (and its last known commit). Reviews carry the
/// repo themselves; the link is what the backend needs for write access.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrLink {
    pub number: u32,
    /// `owner/name`
    pub repo: String,
    /// Last known commit of the branch (saves the diff on the next fetch).
    #[serde(default)]
    pub head_sha: String,
}

impl PrLink {
    pub fn key(&self) -> PrKey {
        PrKey::new(&self.repo, self.number)
    }
}
