//! GitHub REST: Pull Requests lesen und auf `cockpit-core` abbilden.

mod client;
pub mod dto;
mod map;
mod patch;

pub use client::{GitHubClient, Relation};
pub use map::{
    changes_requested, aggregate_checks, check_runs_to_core, comments_with_reviews,
    comments_to_core, mention_for, file_to_diff, pull_to_item, pull_to_review,
};
pub use patch::parse_patch;
