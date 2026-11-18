//! Jira Cloud REST v3: read issues and comments and map them onto `cockpit-core`.

mod adf;
mod client;
pub mod dto;
mod map;

pub use adf::{adf_to_text, adf_to_text_for, text_to_adf};
pub use client::{
    ASSIGNED_OR_WATCHED_JQL, JiraClient, JiraQuery, SPRINT_CLAUSE, is_sprint_query, jql_for,
    jql_for_project, jql_for_selection, open_tickets_jql,
};
pub use map::{comments_to_core, issue_to_item, issue_to_item_with, sprint_of, status_from};
