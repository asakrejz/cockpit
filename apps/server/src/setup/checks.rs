//! Connection and repo checks of the setup page.

use super::*;

pub(super) fn check(cause: JiraCause, name: Option<String>) -> JiraCheck {
    let msg = jira_cause_msg(cause, name.as_deref());
    JiraCheck {
        ok: cause == JiraCause::Ok,
        cause,
        message: msg.de(),
        msg: Some(msg),
        name: name.filter(|n| !n.is_empty()),
    }
}

/// Which cause belongs to an error of Jira.
pub(super) fn jira_cause(f: &Error) -> JiraCause {
    match f {
        Error::NotSignedIn => JiraCause::Token,
        Error::NoPermission => JiraCause::Permission,
        // 404, a foreign page instead of Jira (no JSON) or an unusable address.
        Error::NotFound | Error::Response(_) | Error::Config(_) => JiraCause::Address,
        Error::Network(_) => JiraCause::Reachable,
        _ => JiraCause::Other,
    }
}

impl AppState {
    /// The connection test for Jira with email and API token (never with OAuth): address and email
    /// from the configuration, the token from the .env file. Only `GET /rest/api/3/myself`.
    pub async fn jira_check(&self) -> JiraCheck {
        let s = self.clone();
        let details = tokio::task::spawn_blocking(move || {
            let j = s.sources().jira.clone().filter(|j| !j.site.is_empty());
            j.map(|j| (j.site, j.email, s.token(crate::secrets::JIRA)))
        })
        .await
        .ok()
        .flatten();
        let Some((site, email, Some(token))) = details.filter(|(_, e, _)| !e.is_empty()) else {
            return check(JiraCause::Details, None);
        };
        let client = match JiraClient::new(&site, &email, token) {
            Ok(c) => c,
            Err(e) => return check(jira_cause(&e), None),
        };
        match client.my_name().await {
            Ok(name) => check(JiraCause::Ok, Some(name)),
            Err(e) => {
                tracing::warn!("Jira connection test: {e}");
                check(jira_cause(&e), None)
            }
        }
    }
}

impl AppState {
    /// Looks at each repo, read-only (`GET /repos/{owner}/{name}`): does it exist, can the
    /// key see it, may it push? Changes nothing. At most 20 repos per call.
    pub async fn repos_check(&self, repos: Vec<String>) -> RepoCheckResponse {
        self.oauth_refresh().await;
        let s = self.clone();
        let access = tokio::task::spawn_blocking(move || {
            let api = s
                .sources()
                .github
                .as_ref()
                .map_or(crate::config::DEFAULT_GITHUB_API.to_string(), |g| {
                    g.api_url.clone()
                });
            s.github_access().map(|t| (api, t))
        })
        .await
        .ok()
        .flatten();
        let mut out = Vec::new();
        for repo in repos.into_iter().take(20) {
            out.push(match (&access, crate::agent_git::repo_name_ok(&repo)) {
                (_, false) => repo_check(
                    repo,
                    RepoState::NotFound,
                    None,
                    msg!(ServerSetupRepoInvalid),
                ),
                (None, true) => repo_check(
                    repo,
                    RepoState::NoAccess,
                    None,
                    msg!(ServerSetupGithubNotConnectedYet),
                ),
                (Some((api, token)), true) => {
                    match cockpit_integrations::github::GitHubClient::new(api, token.clone()) {
                        Err(e) => repo_check(
                            repo,
                            RepoState::Failed,
                            None,
                            msg!(ServerSetupGithubAddressInvalid, reason = e.msg()),
                        ),
                        Ok(client) => repo_lookup(&client, repo).await,
                    }
                }
            });
        }
        RepoCheckResponse { repos: out }
    }
}

pub(super) fn repo_check(
    repo: String,
    state: RepoState,
    default_branch: Option<String>,
    message: Msg,
) -> RepoCheck {
    RepoCheck {
        repo,
        state,
        default_branch,
        message: message.de(),
        msg: Some(message),
    }
}

pub(super) async fn repo_lookup(
    client: &cockpit_integrations::github::GitHubClient,
    repo: String,
) -> RepoCheck {
    match client.repo_access(&repo).await {
        Ok(a) if a.archived => repo_check(
            repo,
            RepoState::ReadOnly,
            Some(a.default_branch),
            msg!(ServerSetupRepoArchived),
        ),
        Ok(a) if a.can_push == Some(false) => repo_check(
            repo,
            RepoState::ReadOnly,
            Some(a.default_branch),
            msg!(ServerSetupRepoReadOnly),
        ),
        Ok(a) => repo_check(
            repo,
            RepoState::Ok,
            Some(a.default_branch),
            msg!(ServerSetupRepoOk),
        ),
        Err(Error::NotFound) => repo_check(
            repo,
            RepoState::NotFound,
            None,
            msg!(ServerSetupRepoNotFound),
        ),
        Err(Error::NotSignedIn | Error::NoPermission) => repo_check(
            repo,
            RepoState::NoAccess,
            None,
            msg!(ServerSetupRepoNoPermission),
        ),
        Err(e) => {
            tracing::warn!("Repo check {repo}: {e}");
            repo_check(repo, RepoState::Failed, None, msg!(ServerSetupCheckFailed))
        }
    }
}

/// `POST /api/github/repos/check`: only reads.
pub async fn repos_check(
    State(state): State<AppState>,
    Json(req): Json<RepoCheckRequest>,
) -> Response {
    Json(state.repos_check(req.repos).await).into_response()
}

/// `POST /api/jira/check`: without a body; only reads.
pub async fn jira_check(State(state): State<AppState>) -> Response {
    Json(state.jira_check().await).into_response()
}
