//! Can the connected GitHub access see the configured (watched) repos? Read-only.
//!
//! A GitHub App sign-in only sees repos on accounts where the app is installed. A repo it
//! cannot see answers 404 (or 403 when a permission is missing). This report tells that apart from
//! an empty list: the empty state says "no pull requests" only if everything was reachable.

use cockpit_core::api::{GitHubAccess, RepoSight, RepoVisibility};
use cockpit_integrations::{Cause, Error};

use super::*;

/// What a failed look at a repo means.
pub(super) fn visibility_of(e: &Error) -> RepoVisibility {
    match e.cause() {
        Cause::NotFound => RepoVisibility::NotVisible,
        Cause::Token | Cause::Permission => RepoVisibility::NoPermission,
        _ => RepoVisibility::Unreachable,
    }
}

/// The page where the app is installed on an account or organization.
pub(super) fn install_url(api_url: &str, slug: &str) -> Option<String> {
    let usable = !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    // Only for github.com: the address of an Enterprise host is not known here.
    (usable && api_url.trim_end_matches('/') == crate::config::DEFAULT_GITHUB_API)
        .then(|| format!("https://github.com/apps/{slug}/installations/new"))
}

impl AppState {
    /// The report for `GET /api/github/access`. At most 20 repos, one read call each.
    pub async fn github_access_report(&self) -> GitHubAccess {
        self.oauth_refresh().await;
        let s = self.clone();
        let access = tokio::task::spawn_blocking(move || s.github_assistant_access())
            .await
            .ok()
            .and_then(Result::ok)
            .flatten();
        let Some(access) = access else {
            return GitHubAccess::default();
        };
        let sources = self.sources();
        // Only the configured (watched) repos are checked; without any, the report is just
        // "connected" and no hint appears.
        let g = sources.github_config();
        let (api, repos) = (g.api_url, g.repos);
        let mut report = GitHubAccess {
            connected: true,
            via_app: access.app,
            ..GitHubAccess::default()
        };
        for repo in repos.into_iter().take(20) {
            let visibility = if cockpit_core::repo_name_valid(&repo) {
                match access.client.repo_access(&repo).await {
                    Ok(_) => RepoVisibility::Visible,
                    Err(e) => {
                        tracing::warn!(
                            "GitHub access to {repo}: {}",
                            e.msg().render(cockpit_i18n::Lang::En)
                        );
                        visibility_of(&e)
                    }
                }
            } else {
                RepoVisibility::NotVisible
            };
            report.repos.push(RepoSight { repo, visibility });
        }
        if access.app && report.problem() {
            // Is the app installed anywhere? Without any installation nothing is visible.
            match access.client.installations().await {
                Ok(list) => {
                    report.installed = Some(!list.is_empty());
                    report.install_url = list
                        .iter()
                        .flatten()
                        .find_map(|slug| install_url(&api, slug));
                }
                Err(e) => tracing::warn!("GitHub installations: {e}"),
            }
        }
        report
    }
}

/// `GET /api/github/access`: only reads.
pub async fn github_access_route(State(state): State<AppState>) -> Json<GitHubAccess> {
    Json(state.github_access_report().await)
}
