//! GitHub Projects (v2): the tickets of a project board, read through GraphQL.
//!
//! * [`GitHubClient::list_projects`]: the projects the credential can see (user, organizations,
//!   linked to repos in use) – for the choice in the setup
//! * [`GitHubClient::project_meta`]: one project with its fields
//! * [`GitHubClient::project_items`]: the items, page by page
//! * [`GitHubClient::item_details`]: comments, linked pull requests, parent and sub-issues of
//!   the issues that are new or changed
//! * [`map_item`]: a project item as an inbox entry (pure)
//!
//! The Projects v2 API exists in GraphQL only. The queries are in [`queries`]; the shapes were
//! written from the documented schema and are checked against recorded-shape fixtures only –
//! nothing has been run against the real service.

mod client;
mod dto;
mod map;
pub mod queries;

pub use client::{ItemsPage, MAX_ITEMS, ProjectList, Rate};
pub use dto::{DetailNode, ItemNode};
pub use map::{
    Heavy, MapCtx, apply_details, apply_heavy, heavy_of, iteration_state, map_item, priority_of,
};

use cockpit_core::TokenKind;

/// What the shape of a token tells about its kind, before any answer has come: `github_pat_…` is
/// a fine-grained token, `ghu_…` / `ghs_…` belong to a GitHub App, `ghp_…` is a classic token.
/// (Classic tokens are confirmed by the header `X-OAuth-Scopes` in the first answer.)
pub fn kind_from_prefix(token: &str) -> TokenKind {
    if token.starts_with("github_pat_") {
        TokenKind::FineGrained
    } else if token.starts_with("ghu_") || token.starts_with("ghs_") || token.starts_with("ghr_") {
        TokenKind::App
    } else if token.starts_with("ghp_") {
        TokenKind::Classic
    } else {
        TokenKind::Unknown
    }
}
