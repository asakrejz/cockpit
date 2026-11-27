//! The review panel: the commits of a pull request and the diff of one commit, read from GitHub
//! on demand.
//!
//! The diff of a whole pull request is part of the synced state (`Review::files`); only what the
//! sync does not carry lives here.

use serde::{Deserialize, Serialize};

use crate::review::DiffLine;

/// Lines per file in one answer; the rest is fetched per file ("Load more").
pub const PANEL_FILE_LINES: usize = 300;
/// Upper bound when a single file is asked for in full.
pub const PANEL_MAX_LINES: usize = 5_000;
/// Files per answer; more are cut off and flagged.
pub const PANEL_MAX_FILES: usize = 300;
/// Commits per answer.
pub const PANEL_MAX_COMMITS: usize = 250;

/// One commit of a pull request.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelCommit {
    pub sha: String,
    /// First line of the message.
    pub title: String,
    /// The rest of the message (may be empty).
    #[serde(default)]
    pub body: String,
    /// Login if GitHub knows the account, else the name in the commit.
    pub author: String,
    /// ISO 8601 as GitHub delivers it.
    #[serde(default)]
    pub date: String,
}

/// `GET /api/review/commits?repo=<owner/name>&number=<n>`
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelCommits {
    pub commits: Vec<PanelCommit>,
    /// There are more commits than listed.
    #[serde(default)]
    pub truncated: bool,
}

/// One file of a commit with the start of its diff.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelFile {
    pub name: String,
    /// `added`, `removed`, `modified`, `renamed` …
    #[serde(default)]
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub lines: Vec<DiffLine>,
    /// How many lines the whole diff has; `lines.len()` is smaller when `truncated`.
    #[serde(default)]
    pub total_lines: usize,
    #[serde(default)]
    pub truncated: bool,
    /// GitHub sends no patch (binary or very large file).
    #[serde(default)]
    pub no_patch: bool,
}

/// `GET /api/review/commit?repo=…&number=…&sha=…[&path=…]`
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelCommitDiff {
    pub sha: String,
    pub files: Vec<PanelFile>,
    /// The commit touches more files than listed.
    #[serde(default)]
    pub truncated_files: bool,
}
