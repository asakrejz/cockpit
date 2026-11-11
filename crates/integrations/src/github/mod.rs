//! GitHub REST: Pull Requests lesen und auf `cockpit-core` abbilden.

mod client;
pub mod dto;
mod map;
mod patch;

pub use client::{GitHubClient, Relation};
pub use map::{
    aenderungen_angefordert, aggregate_checks, check_runs_to_core, comments_mit_reviews,
    comments_to_core, erwaehnung_fuer, file_to_diff, pull_to_item, pull_to_review,
};
pub use patch::parse_patch;
