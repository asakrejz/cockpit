//! Client for the GitHub REST API: the connection (base address, token, limits) and the one way
//! out for an outward call, [`GitHubClient::execute`].

use serde::de::DeserializeOwned;

use super::dto::User;
use crate::http::{base_url, client, send};
use crate::outbound::{Outbound, Request, github_comment_request, merge_request, review_request};
use crate::{Error, Secret};

mod checks;
mod pulls;
mod repos;
mod types;

pub use types::{
    LoadedChecks, MAX_REPOS, OpenPulls, PrLimits, Relation, RepoAccess, RepoInfo, RepoList,
};

/// Pages of 100 for files and comments (GitHub delivers at most 3000 files).
const MAX_PAGES: u32 = 3;

pub struct GitHubClient {
    pub(super) http: reqwest::Client,
    pub(super) base: String,
    pub(super) token: Secret,
    limits: PrLimits,
    /// Key of the denial cache: base address and token, never shown.
    scope: u64,
    /// What the answers so far told about the credential (kind, classic scopes).
    pub(super) access: std::sync::Mutex<cockpit_core::Access>,
    /// The GraphQL rate limit as of the last answer.
    pub(super) rate: std::sync::Mutex<Option<super::projects::Rate>>,
}

impl std::fmt::Debug for GitHubClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubClient")
            .field("basis", &self.base)
            .field("token", &self.token)
            .finish()
    }
}

impl GitHubClient {
    /// `api_url` is usually `https://api.github.com` (for GitHub Enterprise
    /// `https://host/api/v3`).
    pub fn new(api_url: &str, token: Secret) -> Result<Self, Error> {
        use std::hash::{Hash, Hasher};
        let base = base_url(api_url)?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        base.hash(&mut hasher);
        token.expose().hash(&mut hasher);
        let kind = super::projects::kind_from_prefix(token.expose());
        Ok(Self {
            http: client()?,
            scope: hasher.finish(),
            base,
            token,
            limits: PrLimits::default(),
            access: std::sync::Mutex::new(cockpit_core::Access { kind, scopes: None }),
            rate: std::sync::Mutex::new(None),
        })
    }

    /// The credential is a sign-in through a GitHub App (user token): its rights are the
    /// permissions of the App, not scopes.
    pub fn with_app(self, app: bool) -> Self {
        if app && let Ok(mut a) = self.access.lock() {
            a.kind = cockpit_core::TokenKind::App;
        }
        self
    }

    /// What is known about the credential: its kind and, for a classic token, its scopes (as far
    /// as an answer has told).
    pub fn access(&self) -> cockpit_core::Access {
        self.access.lock().map(|a| a.clone()).unwrap_or_default()
    }

    /// Changes how many pull requests the sync loads (default: [`PrLimits::default`]).
    pub fn with_limits(mut self, limits: PrLimits) -> Self {
        self.limits = PrLimits {
            per_repo: limits.per_repo.max(1),
            total: limits.total.max(1),
        };
        self
    }

    pub fn limits(&self) -> PrLimits {
        self.limits
    }

    pub(super) fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.token.expose())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    pub(super) fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, path)
    }

    /// Sends the request; a key that the service echoes back in a message
    /// is made unrecognizable.
    pub(super) async fn text(&self, req: reqwest::RequestBuilder) -> Result<String, Error> {
        send(req)
            .await
            .map_err(|e| e.without_key(self.token.expose()))
    }

    pub(super) async fn read_json<T: DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        let text = self.text(req).await?;
        serde_json::from_str(&text).map_err(|e| Error::Response(e.to_string()))
    }

    async fn send(&self, a: &Request) -> Result<String, Error> {
        let method = reqwest::Method::from_bytes(a.method.as_bytes())
            .map_err(|e| Error::Config(e.to_string()))?;
        let mut req = self.request(method, &a.path);
        if let Some(body) = &a.body {
            req = req.json(body);
        }
        self.text(req).await
    }

    /// Login of the key holder.
    pub async fn me(&self) -> Result<String, Error> {
        Ok(self.read_json::<User>(self.get("/user")).await?.login)
    }

    pub(super) async fn pages<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<Vec<T>, Error> {
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let part: Vec<T> = self
                .read_json(
                    self.get(path)
                        .query(&[("per_page", "100"), ("page", &page.to_string())]),
                )
                .await?;
            let full = part.len() == 100;
            all.extend(part);
            if !full {
                break;
            }
        }
        Ok(all)
    }

    /// Executes an outward call. GitHub variants only; Jira is the Jira client's business. Every
    /// variant is named, so a new one does not compile until it says how it is sent.
    pub async fn execute(&self, a: &Outbound) -> Result<(), Error> {
        match a {
            Outbound::GitHubComment { repo, number, text } => self
                .send(&github_comment_request(repo, *number, text))
                .await
                .map(|_| ()),
            Outbound::GitHubReview {
                repo,
                number,
                review,
            } => self
                .send(&review_request(repo, *number, review))
                .await
                .map(|_| ()),
            Outbound::GitHubMerge {
                repo,
                number,
                method,
                sha,
            } => self
                .send(&merge_request(repo, *number, *method, sha))
                .await
                .map(|_| ()),
            Outbound::GitHubBranch { repo, name } => self.branch_create(repo, name).await,
            Outbound::GitHubRecheck {
                repo,
                number,
                check,
            } => self.check_again_start(repo, *number, check).await,
            Outbound::ProjectField {
                project_id,
                item_id,
                field_id,
                value,
            } => {
                self.set_field(project_id, item_id, field_id, value.as_ref())
                    .await
            }
            Outbound::DraftEdit {
                draft_id,
                title,
                body,
            } => self.update_draft(draft_id, title, body).await,
            Outbound::Issue {
                repo,
                number,
                write,
            } => self.issue_write(repo, *number, write).await,
            Outbound::JiraComment { .. } | Outbound::JiraStatus { .. } => {
                Err(Error::Config("not a GitHub call".into()))
            }
        }
    }
}
