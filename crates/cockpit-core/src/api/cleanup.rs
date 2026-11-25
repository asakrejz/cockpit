//! Clean-up of what finished agent runs leave behind (`/api/agent/cleanup`, `/api/agent/branch`).
//!
//! * `GET /api/agent/cleanup`: the preview, [`CleanupPreview`]. Removes nothing.
//! * `POST /api/agent/cleanup`: removes what the body confirms (and only what is still
//!   an orphan), [`CleanupRequest`] → [`CleanupResult`].
//! * `GET /api/agent/branch?run=<key>`: may the branch of the run be deleted, [`BranchCheck`].
//! * `POST /api/agent/branch`: deletes that one branch on GitHub, [`BranchDeleteRequest`] →
//!   [`BranchDeleted`]. Only after the same check; never automatic.

use serde::{Deserialize, Serialize};

/// A container with the run label that belongs to no running run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OrphanContainer {
    pub id: String,
    pub name: String,
}

/// What "Clean up" would remove. The image and containers without the run label are never in it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanupPreview {
    pub containers: Vec<OrphanContainer>,
    /// Folder names under the agent's `runs` folder (names only, never a path).
    pub folders: Vec<String>,
    /// The container program could not be asked (not installed or not running): only folders are listed.
    #[serde(default)]
    pub engine_missing: bool,
}

impl CleanupPreview {
    pub fn is_empty(&self) -> bool {
        self.containers.is_empty() && self.folders.is_empty()
    }
}

/// Body of `POST /api/agent/cleanup`: what the preview showed and the user confirmed.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanupRequest {
    pub containers: Vec<String>,
    pub folders: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanupResult {
    pub containers: usize,
    pub folders: usize,
}

/// May the branch of a finished run be deleted? `reason` says why not.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchCheck {
    pub branch: String,
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<cockpit_i18n::Msg>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchDeleteRequest {
    pub run: String,
    /// The repo of a run in several repos whose branch is meant. Empty = the primary one.
    #[serde(default)]
    pub repo: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchDeleted {
    pub branch: String,
}
