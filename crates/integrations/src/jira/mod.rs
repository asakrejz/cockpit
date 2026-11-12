//! Jira Cloud REST v3: read issues and comments and map them onto `cockpit-core`.

mod adf;
mod client;
pub mod dto;
mod map;

pub use adf::{adf_to_text, adf_to_text_for, text_to_adf};
pub use client::{ASSIGNED_OR_WATCHED_JQL, JiraClient, jql_for, jql_for_project};
pub use map::{comments_to_core, issue_to_item, status_from};
