//! GitHub in the assistant: the list of repos to choose from and the connection test.
//! Both only read and never name the token.

use cockpit_core::api::{
    BranchSuggestion, BranchSuggestionRequest, GitHubCause, GitHubCheck, GitHubRepoInfo,
    GitHubRepoList,
};
use cockpit_core::setup::github_check_text;
use cockpit_integrations::github::{GitHubClient, RepoList};

use super::*;

/// The access to GitHub as the assistant sees it.
struct Access {
    client: GitHubClient,
    /// Signed in with the GitHub App (the list comes from its installations) instead of a token.
    app: bool,
}

/// What the plain text says for a failed call, and the status of our answer.
pub(super) fn github_failure(e: &Error) -> (StatusCode, String) {
    match e {
        Error::NotSignedIn => (
            StatusCode::UNAUTHORIZED,
            github_check_text(GitHubCause::Token, None),
        ),
        Error::NoPermission => (
            StatusCode::FORBIDDEN,
            github_check_text(GitHubCause::Permission, None),
        ),
        Error::NotFound => (
            StatusCode::NOT_FOUND,
            "GitHub kennt diese Adresse nicht. Prüfe die API-Adresse, und bei einer GitHub App, ob sie installiert ist."
                .into(),
        ),
        Error::RateLimit { .. } => (
            StatusCode::TOO_MANY_REQUESTS,
            github_check_text(GitHubCause::RateLimit, None),
        ),
        Error::Network(_) => (
            StatusCode::BAD_GATEWAY,
            github_check_text(GitHubCause::Network, None),
        ),
        other => {
            tracing::warn!("GitHub: {other}");
            (
                StatusCode::BAD_GATEWAY,
                github_check_text(GitHubCause::Other, None),
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
    fn github_assistant_access(&self) -> Result<Option<Access>, Error> {
        let api = self
            .sources()
            .github
            .as_ref()
            .map_or(crate::config::DEFAULT_GITHUB_API.to_string(), |g| {
                g.api_url.clone()
            });
        let (token, app) = match self.oauth_tokens(Service::GitHub) {
            Some(t) => (Some(t.access), true),
            None => (self.token(secrets::GITHUB), false),
        };
        token
            .map(|t| GitHubClient::new(&api, t).map(|client| Access { client, app }))
            .transpose()
    }

    /// The repos the connected access can reach. Read-only.
    pub async fn github_repo_list(&self) -> Result<GitHubRepoList, (StatusCode, String)> {
        self.oauth_refresh().await;
        let s = self.clone();
        let access = tokio::task::spawn_blocking(move || s.github_assistant_access())
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Interner Fehler".to_string(),
                )
            })?
            .map_err(|e| github_failure(&e))?;
        let Some(access) = access else {
            return Err((
                StatusCode::CONFLICT,
                github_check_text(GitHubCause::NotConnected, None),
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
    ) -> Result<BranchSuggestion, (StatusCode, String)> {
        if !crate::agent_git::repo_name_ok(repo) {
            return Err((
                StatusCode::BAD_REQUEST,
                "Das ist kein Repo – gebraucht wird die Form firma/app.".into(),
            ));
        }
        self.oauth_refresh().await;
        let s = self.clone();
        let access = tokio::task::spawn_blocking(move || s.github_assistant_access())
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Interner Fehler".to_string(),
                )
            })?
            .map_err(|e| github_failure(&e))?;
        let Some(access) = access else {
            return Err((
                StatusCode::CONFLICT,
                github_check_text(GitHubCause::NotConnected, None),
            ));
        };
        let found = access
            .client
            .contributing(repo)
            .await
            .map_err(|e| github_failure(&e))?;
        Ok(match found {
            None => BranchSuggestion {
                pattern: None,
                source: None,
                message: format!(
                    "In {repo} gibt es keine Datei CONTRIBUTING.md. Trage das Muster selbst ein oder lass es bei der Voreinstellung."
                ),
            },
            Some((file, text)) => match cockpit_core::branch::suggest(&text) {
                Some(s) => BranchSuggestion {
                    pattern: Some(s.pattern),
                    source: Some(file),
                    message: s.reason,
                },
                None => BranchSuggestion {
                    pattern: None,
                    source: Some(file.clone()),
                    message: format!(
                        "In {file} steht nichts über Branch-Namen, das Cockpit erkennt. Trage das Muster selbst ein oder lass es bei der Voreinstellung."
                    ),
                },
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
        let done = |cause: GitHubCause, user: Option<String>, repos: Vec<RepoCheck>| GitHubCheck {
            ok: cause == GitHubCause::Ok && repos.iter().all(|r| r.state == RepoState::Ok),
            cause,
            message: github_check_text(cause, user.as_deref()),
            user,
            repos,
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
                    "Das ist kein Repo – gebraucht wird die Form firma/app.",
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
        Err((status, message)) => error(status, message),
    }
}

/// `POST /api/github/branch-suggestion`: only reads.
pub async fn branch_suggestion_route(
    State(state): State<AppState>,
    Json(req): Json<BranchSuggestionRequest>,
) -> Response {
    match state.branch_suggestion(req.repo.trim()).await {
        Ok(s) => Json(s).into_response(),
        Err((status, message)) => error(status, message),
    }
}

/// `POST /api/github/check`: only reads.
pub async fn github_check_route(
    State(state): State<AppState>,
    Json(req): Json<RepoCheckRequest>,
) -> Response {
    Json(state.github_check(req.repos).await).into_response()
}
