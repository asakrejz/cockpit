//! Review panel: GitHub's commits and commit files as the shapes of `cockpit_core::api`.

use cockpit_core::api::{
    PANEL_FILE_LINES, PANEL_MAX_COMMITS, PANEL_MAX_FILES, PANEL_MAX_LINES, PanelCommit,
    PanelCommitDiff, PanelCommits, PanelFile,
};

use super::dto::{CommitDetail, PullCommit};
use super::patch::parse_patch;

/// Message → (first line, rest).
fn split_message(message: &str) -> (String, String) {
    let message = message.trim();
    match message.split_once('\n') {
        Some((title, rest)) => (title.trim().into(), rest.trim().into()),
        None => (message.into(), String::new()),
    }
}

/// The commits of a pull request, oldest first, cut at [`PANEL_MAX_COMMITS`].
pub fn panel_commits(list: &[PullCommit]) -> PanelCommits {
    let commits = list
        .iter()
        .take(PANEL_MAX_COMMITS)
        .map(|c| {
            let (title, body) = split_message(&c.commit.message);
            let name = c.commit.author.as_ref();
            PanelCommit {
                sha: c.sha.clone(),
                title,
                body,
                author: c
                    .author
                    .as_ref()
                    .map(|u| u.login.clone())
                    .or_else(|| name.map(|a| a.name.clone()))
                    .unwrap_or_default(),
                date: name.map(|a| a.date.clone()).unwrap_or_default(),
            }
        })
        .collect();
    PanelCommits {
        commits,
        truncated: list.len() > PANEL_MAX_COMMITS,
    }
}

/// The diff of one commit. Without `only` every file gets its first [`PANEL_FILE_LINES`] lines
/// (the rest is flagged); with `only` that one file is returned with up to [`PANEL_MAX_LINES`].
pub fn panel_diff(detail: &CommitDetail, only: Option<&str>) -> PanelCommitDiff {
    let limit = if only.is_some() {
        PANEL_MAX_LINES
    } else {
        PANEL_FILE_LINES
    };
    let selected = detail
        .files
        .iter()
        .filter(|f| only.is_none_or(|p| f.filename == p));
    let files = selected
        .clone()
        .take(PANEL_MAX_FILES)
        .map(|f| {
            let mut lines = f.patch.as_deref().map(parse_patch).unwrap_or_default();
            let total = lines.len();
            lines.truncate(limit);
            PanelFile {
                name: f.filename.clone(),
                status: f.status.clone(),
                additions: f.additions,
                deletions: f.deletions,
                truncated: total > lines.len(),
                total_lines: total,
                lines,
                no_patch: f.patch.is_none(),
            }
        })
        .collect();
    PanelCommitDiff {
        sha: detail.sha.clone(),
        files,
        truncated_files: selected.count() > PANEL_MAX_FILES,
    }
}
