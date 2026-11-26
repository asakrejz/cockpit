//! Which repos to offer first when the agent starts on a ticket. Reads the workspace (earlier runs,
//! pull requests, the ticket text) and the kept list of reachable repos; the ranking itself is
//! pure and lives in `cockpit_core::repos::suggest`.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Query, State};
use cockpit_core::api::RepoSuggestions;
use cockpit_core::repos::suggest::{SuggestInput, suggest_repos};
use cockpit_core::{Kind, Workspace};

use super::*;

/// Earlier runs as `(Jira project, repo)`, oldest first. The repo of a run is remembered by the
/// run itself (in the database); nothing has to be configured.
pub(crate) fn runs_used(ws: &Workspace) -> Vec<(String, String)> {
    ws.runs
        .iter()
        .filter_map(|run| {
            let repo = if run.repo.is_empty() {
                run.options.repo.clone().unwrap_or_default()
            } else {
                run.repo.clone()
            };
            if repo.is_empty() {
                return None;
            }
            let project = ws.item_by_key(&run.ticket_key)?.project()?.to_string();
            Some((project, repo))
        })
        .collect()
}

/// Repos of the pull requests and branches that belong to the ticket.
pub(crate) fn linked_repos(ws: &Workspace, ticket: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |repo: &str| {
        if !repo.is_empty() && !out.iter().any(|r| r == repo) {
            out.push(repo.to_string());
        }
    };
    if let Some(item) = ws.item_by_key(ticket)
        && let Some(pr) = &item.pr
    {
        add(&pr.repo);
    }
    for review in ws
        .reviews
        .iter()
        .filter(|r| r.ticket_key.as_deref() == Some(ticket))
    {
        add(&review.repo);
    }
    // A pull request without a link still names the ticket in its branch or title.
    for item in ws.items.iter().filter(|i| i.kind == Kind::PullRequest) {
        let names = [item.branch.as_deref().unwrap_or_default(), &item.title]
            .iter()
            .any(|t| {
                cockpit_core::link::ticket_keys(t)
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(ticket))
            });
        if names && let Some(pr) = &item.pr {
            add(&pr.repo);
        }
    }
    out
}

/// Title, description and comments of the ticket.
pub(crate) fn ticket_text(ws: &Workspace, ticket: &str) -> String {
    let Some(item) = ws.item_by_key(ticket) else {
        return String::new();
    };
    let mut text = format!("{}\n{}", item.title, item.description);
    for c in &item.comments {
        text.push('\n');
        text.push_str(&c.text);
    }
    text
}

impl AppState {
    /// The suggestions for a ticket, best first.
    pub(crate) fn repo_suggestions(
        &self,
        ticket: &str,
        visible: Option<&[String]>,
    ) -> RepoSuggestions {
        let ws = self.inner.live.copy();
        let repos = self.agent_repos_config();
        let project = ws
            .item_by_key(ticket)
            .and_then(cockpit_core::Item::project)
            .map(str::to_string);
        let issue_repo = cockpit_core::project::parse_issue_key(ticket).map(|(repo, _)| repo);
        let used = runs_used(&ws);
        let linked = linked_repos(&ws, ticket);
        let text = ticket_text(&ws, ticket);
        RepoSuggestions {
            suggestions: suggest_repos(&SuggestInput {
                issue_repo: issue_repo.as_deref(),
                project: project.as_deref(),
                used: &used,
                linked: &linked,
                text: &text,
                repos: Some(&repos),
                visible,
            }),
        }
    }
}

/// `GET /api/agent/repo-suggestions?ticket=PROJ-1`: only reads. Without a reachable GitHub the
/// list of visible repos is unknown and a bare `owner/name` in the text is not suggested.
pub async fn repo_suggestions_route(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<RepoSuggestions> {
    let ticket = q.get("ticket").map(String::as_str).unwrap_or_default();
    let visible: Option<Vec<String>> = state
        .github_repo_list()
        .await
        .ok()
        .map(|l| l.repos.into_iter().map(|r| r.repo).collect());
    Json(state.repo_suggestions(ticket, visible.as_deref()))
}
