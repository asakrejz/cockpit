//! Jira Cloud REST v3: read issues and comments and map them onto `cockpit-core`.

mod adf;
mod client;
mod details;
pub mod dto;
mod jql;
mod map;

pub use adf::{adf_to_text, adf_to_text_for, text_to_adf};
pub use client::{Download, JiraClient, MAX_ISSUES, SearchPage, find_team_field};
pub use details::{FieldPrefs, arrange, attachments_of, details_of};
pub use jql::{
    ASSIGNED_OR_WATCHED_JQL, JiraQuery, SPRINT_CLAUSE, is_sprint_query, jql_for, jql_for_project,
    jql_for_selection, open_tickets_jql,
};
pub use map::{
    comments_to_core, issue_to_item, issue_to_item_full, issue_to_item_with, priority_of,
    sprint_of, status_from, team_of,
};
