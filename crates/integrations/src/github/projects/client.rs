//! The GraphQL calls: one call with its rate limit and error handling. The reading calls built on
//! it are in `read`; writes are in `projects/write.rs`.

use cockpit_core::{Access, Right, TokenKind};
use serde_json::{Value, json};

use super::dto::{Envelope, GqlError, RateLimit};
use crate::github::client::GitHubClient;
use crate::http::{error_from, now_s};
use crate::{Error, time};

mod read;

pub use read::{ItemsPage, MAX_ITEMS, ProjectList};

/// Below this many points left the client stops asking until the limit resets.
const RESERVE: u32 = 15;

/// The GraphQL rate limit as the last answer reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rate {
    pub cost: u32,
    pub remaining: u32,
    /// When the limit resets (seconds since 1970), if the answer said.
    pub reset_s: Option<i64>,
}

impl GitHubClient {
    /// `https://api.github.com/graphql`; GitHub Enterprise Server (`https://host/api/v3`) has
    /// it at `https://host/api/graphql`.
    pub fn graphql_url(&self) -> String {
        match self.base.strip_suffix("/api/v3") {
            Some(host) => format!("{host}/api/graphql"),
            None => format!("{}/graphql", self.base),
        }
    }

    /// The rate limit of the last GraphQL answer.
    pub fn rate(&self) -> Option<Rate> {
        self.rate.lock().ok().and_then(|r| r.clone())
    }

    /// One GraphQL call. `need` is the right the call is about: a refusal is reported as that
    /// right missing. `lenient`: errors of type `NOT_FOUND` do not fail the call (the data
    /// then holds `null` where something is not visible).
    pub(crate) async fn graphql(
        &self,
        query: &str,
        variables: Value,
        need: Right,
        lenient: bool,
    ) -> Result<Value, Error> {
        if let Some(rate) = self.rate()
            && rate.remaining < RESERVE
            && let Some(reset) = rate.reset_s
            && reset > now_s() as i64
        {
            return Err(Error::RateLimit {
                wait_s: Some((reset - now_s() as i64) as u64),
            });
        }
        let req = self
            .http
            .post(self.graphql_url())
            .bearer_auth(self.token.expose())
            .header("Accept", "application/json")
            .json(&json!({ "query": query, "variables": variables }));
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))
            .map_err(|e| e.without_key(self.token.expose()))?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))?;
        self.learn_access(&headers);
        if !status.is_success() {
            let e = error_from(status, &headers, &text, now_s());
            return Err(match e {
                // A 403 here is a refusal of the right, not a quota (that is `RateLimit`).
                Error::NoPermission => self.missing(need),
                other => other.without_key(self.token.expose()),
            });
        }
        let env: Envelope = serde_json::from_str(&text)
            .map_err(|e| Error::Response(e.to_string()))
            .map_err(|e| e.without_key(self.token.expose()))?;
        let data = env.data.unwrap_or(Value::Null);
        if let Some(r) = data
            .get("rateLimit")
            .and_then(|v| serde_json::from_value::<RateLimit>(v.clone()).ok())
            && let Ok(mut slot) = self.rate.lock()
        {
            *slot = Some(Rate {
                cost: r.cost,
                remaining: r.remaining,
                reset_s: time::parse_iso(&r.reset_at),
            });
        }
        let blocking: Vec<&GqlError> = env
            .errors
            .iter()
            .filter(|e| !(lenient && e.kind == "NOT_FOUND" && !data.is_null()))
            .collect();
        if !blocking.is_empty() {
            return Err(self.classify(&blocking, need));
        }
        Ok(data)
    }

    /// Remembers what an answer says about the credential: the scopes of a classic token.
    fn learn_access(&self, headers: &reqwest::header::HeaderMap) {
        let Some(h) = headers.get("x-oauth-scopes").and_then(|v| v.to_str().ok()) else {
            return;
        };
        if let Ok(mut a) = self.access.lock() {
            // A GitHub App token never carries the header; one that does is a classic token.
            if a.kind != TokenKind::App {
                a.kind = TokenKind::Classic;
            }
            a.scopes = Some(cockpit_core::permissions::parse_scopes(h));
        }
    }

    fn missing(&self, right: Right) -> Error {
        Error::MissingRight {
            right,
            kind: self.access().kind,
        }
    }

    /// Turns the errors of a GraphQL answer into one [`Error`].
    fn classify(&self, errors: &[&GqlError], need: Right) -> Error {
        let kind_is = |k: &str| errors.iter().any(|e| e.kind == k);
        let says = |w: &str| {
            errors
                .iter()
                .any(|e| e.message.to_lowercase().contains(&w.to_lowercase()))
        };
        if kind_is("RATE_LIMITED") || says("rate limit") {
            let wait = self
                .rate()
                .and_then(|r| r.reset_s)
                .map(|t| (t - now_s() as i64).max(1) as u64)
                .or(Some(60));
            return Error::RateLimit { wait_s: wait };
        }
        if let Some(e) = errors.iter().find(|e| e.kind == "INSUFFICIENT_SCOPES") {
            return Error::MissingRight {
                right: right_from_scope_message(&e.message, need),
                kind: TokenKind::Classic,
            };
        }
        if let Some(e) = errors
            .iter()
            .find(|e| e.kind == "FORBIDDEN" || e.message.contains("Resource not accessible"))
        {
            // A refusal below `content` is about the issue behind the item, not the project.
            let right = if e.path.iter().any(|p| p.as_str() == Some("content")) {
                match need {
                    Right::ProjectsWrite | Right::IssuesWrite => Right::IssuesWrite,
                    _ => Right::IssuesRead,
                }
            } else {
                need
            };
            return self.missing(right);
        }
        if kind_is("NOT_FOUND") {
            return Error::NotFound;
        }
        let first = errors
            .first()
            .map(|e| e.message.clone())
            .unwrap_or_default();
        Error::Rejected(first)
    }

    /// The login and the access of the credential: one cheap REST call that also returns the
    /// scopes of a classic token in `X-OAuth-Scopes`. Read only.
    pub async fn probe_access(&self) -> Result<(String, Access), Error> {
        let resp = self
            .get("/user")
            .send()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))
            .map_err(|e| e.without_key(self.token.expose()))?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))?;
        self.learn_access(&headers);
        if !status.is_success() {
            return Err(error_from(status, &headers, &text, now_s()));
        }
        let login = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["login"].as_str().map(str::to_string))
            .unwrap_or_default();
        Ok((login, self.access()))
    }

    /// People who can be assigned to issues of `repo` (`GET /repos/{repo}/assignees`) whose login
    /// contains `query` (case-insensitive), at most 30. Read only.
    pub async fn assignees(&self, repo: &str, query: &str) -> Result<Vec<String>, Error> {
        if !cockpit_core::repo_name_valid(repo) {
            return Err(Error::Config(format!("not a repo: {repo}")));
        }
        #[derive(serde::Deserialize)]
        struct Login {
            login: String,
        }
        let people: Vec<Login> = self.pages(&format!("/repos/{repo}/assignees")).await?;
        let q = query.trim().to_lowercase();
        Ok(people
            .into_iter()
            .map(|p| p.login)
            .filter(|l| q.is_empty() || l.to_lowercase().contains(&q))
            .take(30)
            .collect())
    }
}

/// Which right an `INSUFFICIENT_SCOPES` message asks for: it names the scopes the field needs
/// (`… requires one of the following scopes: ['read:project'] …`).
pub(crate) fn right_from_scope_message(message: &str, need: Right) -> Right {
    let wanted = message
        .split_once("scopes: [")
        .map(|(_, rest)| rest.split(']').next().unwrap_or(""))
        .unwrap_or("");
    if wanted.contains("'read:project'") {
        Right::ProjectsRead
    } else if wanted.contains("'project'") {
        Right::ProjectsWrite
    } else if wanted.contains("'repo'") || wanted.contains("'public_repo'") {
        match need {
            Right::ProjectsWrite | Right::IssuesWrite => Right::IssuesWrite,
            _ => Right::IssuesRead,
        }
    } else {
        need
    }
}
