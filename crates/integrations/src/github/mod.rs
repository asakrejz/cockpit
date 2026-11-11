//! GitHub REST: Pull Requests lesen und auf `cockpit-core` abbilden.

mod client;
pub mod dto;
mod map;
mod patch;

pub use client::{GitHubClient, Relation};
pub use map::{aggregate_checks, comments_to_core, file_to_diff, pull_to_item, pull_to_review};
pub use patch::parse_patch;
