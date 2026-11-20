//! GitHub REST: read pull requests and map them onto `cockpit-core`.

mod client;
pub mod dto;
pub mod followup;
mod map;
mod panel;
mod patch;
pub mod projects;

pub use client::{
    GitHubClient, LoadedChecks, MAX_REPOS, OpenPulls, PrLimits, Relation, RepoAccess, RepoInfo,
    RepoList,
};
pub use map::{
    PrRelation, aggregate_checks, changes_requested, check_runs_to_core, comments_to_core,
    comments_with_reviews, file_to_diff, mention_for, pull_to_item, pull_to_item_for,
    pull_to_review, status_to_checks,
};
pub use panel::{panel_commits, panel_diff};
pub use patch::parse_patch;
