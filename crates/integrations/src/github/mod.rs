//! GitHub REST: read pull requests and map them onto `cockpit-core`.

mod client;
pub mod dto;
pub mod followup;
mod map;
mod patch;

pub use client::{GitHubClient, Relation, RepoAccess};
pub use map::{
    aggregate_checks, changes_requested, check_runs_to_core, comments_to_core,
    comments_with_reviews, file_to_diff, mention_for, pull_to_item, pull_to_review,
};
pub use patch::parse_patch;
