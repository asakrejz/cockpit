//! Reading client for the GitHub REST API.

use cockpit_core::Item;

use serde::de::DeserializeOwned;

use super::dto::{
    Annotation, CheckRun, CheckRuns, CommitDetail, ContentFile, GitRefResponse, InstallationRepos,
    Installations, IssueComment, Pull, PullCommit, PullFile, PullReview, PullSummary, Repo,
    RepoListEntry, ReviewComment, SearchItem, SearchResult, User,
};
use super::followup::{self, FailedCheck, ReviewInput};
use super::map;
use crate::http::{base_url, client, send};
use crate::outbound::{Outbound, Request, branch_request, check_again_request};
use crate::{Error, Secret};

/// Why a pull request is in the inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// A review from me is requested.
    ReviewRequested,
    /// The pull request is mine.
    Own,
}

impl Relation {
    fn search(self) -> &'static str {
        match self {
            Relation::ReviewRequested => "is:pr is:open archived:false review-requested:@me",
            Relation::Own => "is:pr is:open archived:false author:@me",
        }
    }
}

/// What a key may do in a repo (see [`GitHubClient::repo_access`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoAccess {
    pub default_branch: String,
    pub archived: bool,
    /// `None` if the service does not say.
    pub can_push: Option<bool>,
}

/// One repo of the list the user can choose from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoInfo {
    /// `owner/name`
    pub repo: String,
    pub private: bool,
    pub archived: bool,
    pub default_branch: String,
    /// `None` if the service does not say.
    pub can_push: Option<bool>,
}

/// The repos a key can reach. `truncated`: there are more than [`MAX_REPOS`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoList {
    pub repos: Vec<RepoInfo>,
    pub truncated: bool,
}

/// At most this many repos are listed.
pub const MAX_REPOS: usize = 500;

impl From<RepoListEntry> for RepoInfo {
    fn from(r: RepoListEntry) -> Self {
        Self {
            repo: r.full_name,
            private: r.private,
            archived: r.archived,
            default_branch: r
                .default_branch
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| "main".into()),
            can_push: r.permissions.map(|p| p.push),
        }
    }
}

/// At most this many pull requests per relation are loaded per fetch (the searches).
const MAX_PRS: usize = 30;

/// How many open pull requests the sync loads at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrLimits {
    /// Per repo (the most recently updated ones come first).
    pub per_repo: usize,
    /// Over all repos together.
    pub total: usize,
}

impl PrLimits {
    pub const DEFAULT_PER_REPO: usize = 200;
    pub const DEFAULT_TOTAL: usize = 500;
}

impl Default for PrLimits {
    fn default() -> Self {
        Self {
            per_repo: Self::DEFAULT_PER_REPO,
            total: Self::DEFAULT_TOTAL,
        }
    }
}

/// The open pull requests of one repo.
#[derive(Clone, Debug)]
pub struct OpenPulls {
    pub pulls: Vec<Pull>,
    /// There are more open pull requests than were loaded.
    pub truncated: bool,
}
/// Pages of 100 for files and comments (GitHub delivers at most 3000 files).
const MAX_PAGES: u32 = 3;

pub struct GitHubClient {
    http: reqwest::Client,
    base: String,
    token: Secret,
    limits: PrLimits,
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
        Ok(Self {
            http: client()?,
            base: base_url(api_url)?,
            token,
            limits: PrLimits::default(),
        })
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

    /// The open pull requests of a repo, most recently updated first, page by page (100 each)
    /// until `max` is reached. One call per page, none per pull request. Only reads.
    pub async fn pulls_open(&self, repo: &str, max: usize) -> Result<OpenPulls, Error> {
        let max = max.max(1);
        let mut all: Vec<Pull> = Vec::new();
        let mut page = 1u32;
        loop {
            let part: Vec<Pull> = self
                .lies_json(self.get(&format!("/repos/{repo}/pulls")).query(&[
                    ("state", "open"),
                    ("sort", "updated"),
                    ("direction", "desc"),
                    ("per_page", "100"),
                    ("page", &page.to_string()),
                ]))
                .await?;
            let full = part.len() >= 100;
            all.extend(part);
            // One more than `max` tells that there are more.
            if all.len() > max || !full {
                break;
            }
            page += 1;
        }
        let truncated = all.len() > max;
        all.truncate(max);
        Ok(OpenPulls {
            pulls: all,
            truncated,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.token.expose())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, path)
    }

    /// Sends the request; a key that the service echoes back in a message
    /// is made unrecognizable.
    async fn text(&self, req: reqwest::RequestBuilder) -> Result<String, Error> {
        send(req)
            .await
            .map_err(|e| e.without_key(self.token.expose()))
    }

    async fn lies_json<T: DeserializeOwned>(
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
        Ok(self.lies_json::<User>(self.get("/user")).await?.login)
    }

    /// Open pull requests for which a review from me is requested or which belong to me.
    /// Returns `(owner/name, number)`.
    pub async fn search(&self, relation: Relation) -> Result<Vec<(String, u32)>, Error> {
        self.search_with(relation.search()).await
    }

    /// Like [`GitHubClient::search`], restricted to one repo (`owner/name`).
    pub async fn search_in(
        &self,
        relation: Relation,
        repo: &str,
    ) -> Result<Vec<(String, u32)>, Error> {
        self.search_with(&format!("{} repo:{repo}", relation.search()))
            .await
    }

    /// The search, page by page (100 each), until there is no full page any more or
    /// [`MAX_PRS`] is reached.
    async fn search_with(&self, q: &str) -> Result<Vec<(String, u32)>, Error> {
        let per_page = MAX_PRS.min(100);
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let res: SearchResult = self
                .lies_json(self.get("/search/issues").query(&[
                    ("q", q),
                    ("sort", "updated"),
                    ("per_page", &per_page.to_string()),
                    ("page", &page.to_string()),
                ]))
                .await?;
            let full = res.items.len() >= per_page;
            all.extend(
                res.items
                    .iter()
                    .filter_map(|i: &SearchItem| Some((i.repo()?, i.number))),
            );
            if !full || all.len() >= MAX_PRS {
                break;
            }
        }
        all.truncate(MAX_PRS);
        Ok(all)
    }

    pub async fn pull(&self, repo: &str, nr: u32) -> Result<Pull, Error> {
        self.lies_json(self.get(&format!("/repos/{repo}/pulls/{nr}")))
            .await
    }

    /// Changed files with patch.
    pub async fn files(&self, repo: &str, nr: u32) -> Result<Vec<PullFile>, Error> {
        self.pages(&format!("/repos/{repo}/pulls/{nr}/files")).await
    }

    /// The commits of a pull request (oldest first; GitHub delivers at most 250).
    pub async fn commits(&self, repo: &str, nr: u32) -> Result<Vec<PullCommit>, Error> {
        self.pages(&format!("/repos/{repo}/pulls/{nr}/commits"))
            .await
    }

    /// One commit with its files and patches (GitHub lists at most 300 files here).
    pub async fn commit(&self, repo: &str, sha: &str) -> Result<CommitDetail, Error> {
        self.lies_json(self.get(&format!("/repos/{repo}/commits/{sha}")))
            .await
    }

    pub async fn comments(&self, repo: &str, nr: u32) -> Result<Vec<IssueComment>, Error> {
        self.pages(&format!("/repos/{repo}/issues/{nr}/comments"))
            .await
    }

    pub async fn line_comments(&self, repo: &str, nr: u32) -> Result<Vec<ReviewComment>, Error> {
        self.pages(&format!("/repos/{repo}/pulls/{nr}/comments"))
            .await
    }

    /// Status of the checks for a commit.
    pub async fn checks(&self, repo: &str, sha: &str) -> Result<cockpit_core::Checks, Error> {
        Ok(map::aggregate_checks(&self.check_runs(repo, sha).await?))
    }

    /// The individual check runs of a commit.
    pub async fn check_runs(&self, repo: &str, sha: &str) -> Result<Vec<CheckRun>, Error> {
        let runs: CheckRuns = self
            .lies_json(
                self.get(&format!("/repos/{repo}/commits/{sha}/check-runs"))
                    .query(&[("per_page", "100")]),
            )
            .await?;
        Ok(runs.check_runs)
    }

    /// Verdicts of the reviewers.
    pub async fn reviews(&self, repo: &str, nr: u32) -> Result<Vec<PullReview>, Error> {
        self.pages(&format!("/repos/{repo}/pulls/{nr}/reviews"))
            .await
    }

    async fn pages<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<Vec<T>, Error> {
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let part: Vec<T> = self
                .lies_json(
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

    /// Inbox entries: first review requests, then own pull requests (without duplicates).
    /// For each pull request itself and the check status are loaded afterwards.
    pub async fn load_inbox(&self, now: i64) -> Result<Vec<Item>, Error> {
        let mut items: Vec<Item> = Vec::new();
        let mut seen: Vec<(String, u32)> = Vec::new();
        for relation in [Relation::ReviewRequested, Relation::Own] {
            for (repo, nr) in self.search(relation).await? {
                if seen.contains(&(repo.clone(), nr)) {
                    continue;
                }
                let pull = self.pull(&repo, nr).await?;
                let checks = self.checks(&repo, &pull.head.sha).await?;
                items.push(map::pull_to_item(&repo, &pull, checks, relation, now));
                seen.push((repo, nr));
            }
        }
        Ok(items)
    }
}

// ---------- Inputs for follow-up jobs (read only) ----------

impl GitHubClient {
    /// Review threads and review texts of a pull request – the input for „implement review
    /// comments“. Only reads.
    pub async fn review_input(&self, repo: &str, nr: u32) -> Result<ReviewInput, Error> {
        let lines = self.line_comments(repo, nr).await?;
        let reviews = self.reviews(repo, nr).await?;
        Ok(ReviewInput {
            threads: followup::threads(&lines),
            notes: followup::notes(&reviews),
        })
    }

    /// The red checks on the current commit of the pull request with a shortened excerpt of their
    /// output and annotations – the input for „fix red checks“. Only reads. The annotations are
    /// optional: if GitHub does not deliver them, the output alone counts.
    pub async fn failed_checks(&self, repo: &str, nr: u32) -> Result<Vec<FailedCheck>, Error> {
        let pull = self.pull(repo, nr).await?;
        let runs = self.check_runs(repo, &pull.head.sha).await?;
        let mut out = Vec::new();
        for run in runs
            .iter()
            .filter(|r| followup::is_failed(r))
            .take(followup::MAX_CHECKS)
        {
            let annotations: Vec<Annotation> = self
                .lies_json(
                    self.get(&format!("/repos/{repo}/check-runs/{}/annotations", run.id))
                        .query(&[("per_page", "50")]),
                )
                .await
                .unwrap_or_default();
            out.push(FailedCheck {
                name: run.name.clone(),
                excerpt: followup::excerpt(run, &annotations),
            });
        }
        Ok(out)
    }

    /// The open pull request whose branch is `branch` in the same repo, if there is one.
    pub async fn pull_for_branch(
        &self,
        repo: &str,
        branch: &str,
    ) -> Result<Option<PullSummary>, Error> {
        let owner = repo.split('/').next().unwrap_or_default();
        let list: Vec<PullSummary> = self
            .lies_json(self.get(&format!("/repos/{repo}/pulls")).query(&[
                ("head", format!("{owner}:{branch}").as_str()),
                ("state", "open"),
                ("per_page", "5"),
            ]))
            .await?;
        // The filter of GitHub is exact; the check protects against a differently behaving server.
        Ok(list.into_iter().find(|p| {
            p.head
                .as_ref()
                .is_none_or(|h| h.name == branch || h.name.is_empty())
        }))
    }
}

// ---------- Writing ----------

impl GitHubClient {
    /// Creates a pull request from `head` against `base`. **Only after your approval** – the
    /// caller has to look for an existing one first ([`GitHubClient::pull_for_branch`]).
    /// Neither merges nor changes anything else.
    pub async fn pull_create(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<PullSummary, Error> {
        let text = self
            .send(&crate::outbound::pull_request_request(
                repo, head, base, title, body, draft,
            ))
            .await?;
        serde_json::from_str(&text).map_err(|e| Error::Response(e.to_string()))
    }
}

impl GitHubClient {
    /// Executes an outbound call. GitHub variants only; Jira is the Jira client's business.
    pub async fn execute(&self, a: &Outbound) -> Result<(), Error> {
        match a {
            Outbound::GitHubBranch { repo, name } => self.branch_create(repo, name).await,
            Outbound::GitHubRecheck {
                repo,
                number,
                check,
            } => self.check_again_start(repo, *number, check).await,
            other => match other.request() {
                Some(request) => self.send(&request).await.map(|_| ()),
                None => Err(Error::Config("Kein GitHub-Aufruf".into())),
            },
        }
    }

    /// Creates `name` as a ref from the default branch of the repo.
    pub async fn branch_create(&self, repo: &str, name: &str) -> Result<(), Error> {
        let r: Repo = self.lies_json(self.get(&format!("/repos/{repo}"))).await?;
        let r#ref: GitRefResponse = self
            .lies_json(self.get(&format!("/repos/{repo}/git/ref/heads/{}", r.default_branch)))
            .await?;
        self.send(&branch_request(repo, name, &r#ref.object.sha))
            .await
            .map(|_| ())
    }

    /// Read-only look at a repo: does it exist, may the key holder see it and push to it?
    /// Changes nothing.
    pub async fn repo_access(&self, repo: &str) -> Result<RepoAccess, Error> {
        let r: Repo = self.lies_json(self.get(&format!("/repos/{repo}"))).await?;
        Ok(RepoAccess {
            default_branch: r.default_branch,
            archived: r.archived,
            can_push: r.permissions.map(|p| p.push),
        })
    }

    /// The contribution guidelines of a repo: the first of `CONTRIBUTING.md`,
    /// `.github/CONTRIBUTING.md` and `docs/CONTRIBUTING.md` that exists (`GET
    /// /repos/{repo}/contents/{path}`). Read-only. `None` if there is none; files over 200 kB
    /// are skipped. Returns the path and the text.
    pub async fn contributing(&self, repo: &str) -> Result<Option<(String, String)>, Error> {
        for path in cockpit_core::branch::GUIDELINE_FILES {
            let file: Result<ContentFile, Error> = self
                .lies_json(self.get(&format!("/repos/{repo}/contents/{path}")))
                .await;
            let file = match file {
                Ok(f) => f,
                // No such file: try the next place.
                // A folder with that name answers with a list, not a file.
                Err(Error::NotFound | Error::Response(_)) => continue,
                Err(e) => return Err(e),
            };
            if file.kind != "file" || file.encoding != "base64" || file.size > 200_000 {
                continue;
            }
            use base64::Engine as _;
            let clean: String = file
                .content
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(clean) else {
                continue;
            };
            return Ok(Some((
                path.to_string(),
                String::from_utf8_lossy(&bytes).into_owned(),
            )));
        }
        Ok(None)
    }

    /// The repos a personal access token can reach (`GET /user/repos`): own, as collaborator
    /// and through organizations. Read-only; at most [`MAX_REPOS`], sorted by name.
    pub async fn repo_list(&self) -> Result<RepoList, Error> {
        let mut list = RepoList::default();
        for page in 1..=(MAX_REPOS / 100 + 1) {
            let part: Vec<RepoListEntry> = self
                .lies_json(self.get("/user/repos").query(&[
                    ("affiliation", "owner,collaborator,organization_member"),
                    ("sort", "full_name"),
                    ("per_page", "100"),
                    ("page", &page.to_string()),
                ]))
                .await?;
            let full = part.len() == 100;
            list.repos.extend(part.into_iter().map(RepoInfo::from));
            if !full {
                break;
            }
        }
        list_finish(&mut list);
        Ok(list)
    }

    /// The repos a GitHub App sign-in can reach: all installations of the app for the user
    /// (`GET /user/installations`) and their repos. Read-only.
    pub async fn installation_repo_list(&self) -> Result<RepoList, Error> {
        let installations: Installations = self
            .lies_json(
                self.get("/user/installations")
                    .query(&[("per_page", "100")]),
            )
            .await?;
        let mut list = RepoList::default();
        for i in installations.installations {
            for page in 1..=(MAX_REPOS / 100 + 1) {
                let part: InstallationRepos = self
                    .lies_json(
                        self.get(&format!("/user/installations/{}/repositories", i.id))
                            .query(&[("per_page", "100"), ("page", &page.to_string())]),
                    )
                    .await?;
                let full = part.repositories.len() == 100;
                list.repos
                    .extend(part.repositories.into_iter().map(RepoInfo::from));
                if !full {
                    break;
                }
            }
        }
        list_finish(&mut list);
        Ok(list)
    }

    /// Re-requests the check with this name on the current commit of the pull request.
    pub async fn check_again_start(&self, repo: &str, nr: u32, check: &str) -> Result<(), Error> {
        let pull = self.pull(repo, nr).await?;
        let runs = self.check_runs(repo, &pull.head.sha).await?;
        let id = runs
            .iter()
            .find(|c| c.name == check)
            .map(|c| c.id)
            .ok_or_else(|| {
                Error::NotPossible(format!("Check „{check}“ gibt es bei GitHub nicht (mehr)"))
            })?;
        self.send(&check_again_request(repo, id)).await.map(|_| ())
    }
}

/// Sorts by name, drops duplicates and applies the limit.
fn list_finish(list: &mut RepoList) {
    list.repos.sort_by_key(|r| r.repo.to_lowercase());
    list.repos.dedup_by(|a, b| a.repo == b.repo);
    if list.repos.len() > MAX_REPOS {
        list.repos.truncate(MAX_REPOS);
        list.truncated = true;
    }
}
