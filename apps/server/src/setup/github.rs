//! GitHub in the assistant: the list of repos to choose from and the connection test.
//! Both only read and never name the token.

use cockpit_core::api::{
    BranchSuggestion, BranchSuggestionRequest, GitHubCause, GitHubCheck, GitHubRepoInfo,
    GitHubRepoList,
};
use cockpit_integrations::github::{GitHubClient, RepoList};

use super::*;

/// The access to GitHub as the assistant sees it.
pub(super) struct Access {
    pub(super) client: GitHubClient,
    /// Signed in with the GitHub App (the list comes from its installations) instead of a token.
    pub(super) app: bool,
}

/// What the plain text says for a failed call, and the status of our answer.
pub(super) fn github_failure(e: &Error) -> (StatusCode, Msg) {
    match e {
        Error::NotSignedIn => (
            StatusCode::UNAUTHORIZED,
            github_cause_msg(GitHubCause::Token, None),
        ),
        Error::NoPermission => (
            StatusCode::FORBIDDEN,
            github_cause_msg(GitHubCause::Permission, None),
        ),
        Error::NotFound => (StatusCode::NOT_FOUND, msg!(ServerSetupGithubUnknownAddress)),
        Error::RateLimit { .. } => (
            StatusCode::TOO_MANY_REQUESTS,
            github_cause_msg(GitHubCause::RateLimit, None),
        ),
        Error::Network(_) => (
            StatusCode::BAD_GATEWAY,
            github_cause_msg(GitHubCause::Network, None),
        ),
        other => {
            tracing::warn!("GitHub: {other}");
            (
                StatusCode::BAD_GATEWAY,
                github_cause_msg(GitHubCause::Other, None),
            )
        }
    }
}

fn cause(e: &Error) -> GitHubCause {
    match e {
        Error::NotSignedIn => GitHubCause::Token,
        Error::NoPermission => GitHubCause::Permission,
        Error::RateLimit { .. } => GitHubCause::RateLimit,
        Error::Network(_) => GitHubCause::Network,
        _ => GitHubCause::Other,
    }
}

fn info(list: RepoList, via: &str) -> GitHubRepoList {
    GitHubRepoList {
        repos: list
            .repos
            .into_iter()
            .map(|r| GitHubRepoInfo {
                repo: r.repo,
                private: r.private,
                archived: r.archived,
                default_branch: r.default_branch,
                can_push: r.can_push,
            })
            .collect(),
        truncated: list.truncated,
        via: via.into(),
    }
}

impl AppState {
    /// Client for the current access (sign-in via the app before the token), or `None`
    /// if GitHub is not connected. Blocking (.env file).
    pub(super) fn github_assistant_access(&self) -> Result<Option<Access>, Error> {
        let api = self
            .sources()
            .github
            .as_ref()
            .map_or(crate::config::DEFAULT_GITHUB_API.to_string(), |g| {
                g.api_url.clone()
            });
        self.github_credentials()
            .map(|(t, app)| GitHubClient::new(&api, t).map(|client| Access { client, app }))
            .transpose()
    }

    /// How long the list of reachable repos is kept before GitHub is asked again.
    const REPO_LIST_VALID: std::time::Duration = std::time::Duration::from_secs(300);

    /// Forgets the kept list of reachable repos (the access changed).
    pub(crate) fn repo_list_forget(&self) {
        *self
            .inner
            .repo_list
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// The kept list, if it is still fresh.
    pub(crate) fn repo_list_kept(&self) -> Option<GitHubRepoList> {
        let kept = self
            .inner
            .repo_list
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        kept.as_ref()
            .filter(|(at, _)| at.elapsed() < Self::REPO_LIST_VALID)
            .map(|(_, list)| list.clone())
    }

    /// The repos the connected access can reach: `/user/repos` for a token, the installation
    /// repos for the GitHub App. Kept for a few minutes. Read-only.
    pub async fn github_repo_list(&self) -> Result<GitHubRepoList, (StatusCode, Msg)> {
        if let Some(list) = self.repo_list_kept() {
            return Ok(list);
        }
        let list = self.github_repo_list_fresh().await?;
        *self
            .inner
            .repo_list
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some((std::time::Instant::now(), list.clone()));
        Ok(list)
    }

    async fn github_repo_list_fresh(&self) -> Result<GitHubRepoList, (StatusCode, Msg)> {
        self.oauth_refresh().await;
        let s = self.clone();
        let access = tokio::task::spawn_blocking(move || s.github_assistant_access())
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    msg!(ServerSetupInternalError),
                )
            })?
            .map_err(|e| github_failure(&e))?;
        let Some(access) = access else {
            return Err((
                StatusCode::CONFLICT,
                github_cause_msg(GitHubCause::NotConnected, None),
            ));
        };
        let (list, via) = if access.app {
            (access.client.installation_repo_list().await, "app")
        } else {
            (access.client.repo_list().await, "token")
        };
        list.map(|l| info(l, via)).map_err(|e| github_failure(&e))
    }

    /// Reads the contribution guidelines of `repo` (read-only) and proposes a branch pattern.
    /// Changes nothing; the user confirms the proposal.
    pub async fn branch_suggestion(
        &self,
        repo: &str,
    ) -> Result<BranchSuggestion, (StatusCode, Msg)> {
        if !crate::agent_git::repo_name_ok(repo) {
            return Err((StatusCode::BAD_REQUEST, msg!(ServerSetupRepoInvalid)));
        }
        self.oauth_refresh().await;
        let s = self.clone();
        let access = tokio::task::spawn_blocking(move || s.github_assistant_access())
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    msg!(ServerSetupInternalError),
                )
            })?
            .map_err(|e| github_failure(&e))?;
        let Some(access) = access else {
            return Err((
                StatusCode::CONFLICT,
                github_cause_msg(GitHubCause::NotConnected, None),
            ));
        };
        let found = access
            .client
            .contributing(repo)
            .await
            .map_err(|e| github_failure(&e))?;
        Ok(match found {
            None => {
                let m = msg!(ServerSetupBranchNoContributing, repo = repo);
                BranchSuggestion {
                    pattern: None,
                    source: None,
                    message: m.de(),
                    msg: Some(m),
                }
            }
            Some((file, text)) => match cockpit_core::branch::suggest(&text) {
                Some(s) => BranchSuggestion {
                    pattern: Some(s.pattern),
                    source: Some(file),
                    // The reason comes from the core as German text.
                    message: s.reason,
                    msg: None,
                },
                None => {
                    let m = msg!(ServerSetupBranchNothingFound, file = &file);
                    BranchSuggestion {
                        pattern: None,
                        source: Some(file),
                        message: m.de(),
                        msg: Some(m),
                    }
                }
            },
        })
    }

    /// The connection test: who GitHub sees (`GET /user`) and, for every chosen repo,
    /// `GET /repos/{owner}/{name}`. Only reads; at most 20 repos.
    pub async fn github_check(&self, repos: Vec<String>) -> GitHubCheck {
        self.oauth_refresh().await;
        let s = self.clone();
        let access = tokio::task::spawn_blocking(move || s.github_assistant_access())
            .await
            .ok()
            .and_then(Result::ok)
            .flatten();
        let done = |cause: GitHubCause, user: Option<String>, repos: Vec<RepoCheck>| {
            let m = github_cause_msg(cause, user.as_deref());
            GitHubCheck {
                ok: cause == GitHubCause::Ok && repos.iter().all(|r| r.state == RepoState::Ok),
                cause,
                message: m.de(),
                msg: Some(m),
                user,
                repos,
            }
        };
        let Some(access) = access else {
            return done(GitHubCause::NotConnected, None, Vec::new());
        };
        // A GitHub App sign-in may not call `/user`; its answer is still the login.
        let user = match access.client.me().await {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!("GitHub connection test: {e}");
                return done(cause(&e), None, Vec::new());
            }
        };
        let mut out = Vec::new();
        for repo in repos.into_iter().take(20) {
            out.push(if crate::agent_git::repo_name_ok(&repo) {
                repo_lookup(&access.client, repo).await
            } else {
                repo_check(
                    repo,
                    RepoState::NotFound,
                    None,
                    msg!(ServerSetupRepoInvalid),
                )
            });
        }
        done(GitHubCause::Ok, Some(user), out)
    }
}

/// `GET /api/github/repos`: only reads.
pub async fn repos_list(State(state): State<AppState>) -> Response {
    match state.github_repo_list().await {
        Ok(list) => Json(list).into_response(),
        Err((status, message)) => error_msg(status, message),
    }
}

/// `POST /api/github/branch-suggestion`: only reads.
pub async fn branch_suggestion_route(
    State(state): State<AppState>,
    Json(req): Json<BranchSuggestionRequest>,
) -> Response {
    match state.branch_suggestion(req.repo.trim()).await {
        Ok(s) => Json(s).into_response(),
        Err((status, message)) => error_msg(status, message),
    }
}

/// `POST /api/github/check`: only reads.
pub async fn github_check_route(
    State(state): State<AppState>,
    Json(req): Json<RepoCheckRequest>,
) -> Response {
    Json(state.github_check(req.repos).await).into_response()
}
