//! Pull requests: lists, searches, one pull request with its files, commits and comments.
//! Reading only, except [`GitHubClient::pull_create`].

use cockpit_core::Item;

use super::super::dto::{
    CommitDetail, IssueComment, Pull, PullCommit, PullFile, PullReview, PullSummary, ReviewComment,
    SearchItem, SearchResult,
};
use super::super::followup::{self, ReviewInput};
use super::super::map;
use super::{GitHubClient, MAX_PAGES, OpenPulls, Relation};
use crate::Error;

/// At most this many pull requests per relation are loaded per fetch (the searches).
const MAX_PRS: usize = 30;

impl GitHubClient {
    /// The open pull requests of a repo, most recently updated first, page by page (100 each)
    /// until `max` is reached. One call per page, none per pull request. Only reads.
    pub async fn pulls_open(&self, repo: &str, max: usize) -> Result<OpenPulls, Error> {
        let max = max.max(1);
        let mut all: Vec<Pull> = Vec::new();
        let mut page = 1u32;
        loop {
            let part: Vec<Pull> = self
                .read_json(self.get(&format!("/repos/{repo}/pulls")).query(&[
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

    /// One search for pull requests (`is:pr is:open …`) across every repo the key can see, at most
    /// [`MAX_PRS`] results, most recently updated first. Returns `(owner/name, number)`.
    pub async fn search_pulls(&self, query: &str) -> Result<Vec<(String, u32)>, Error> {
        self.search_with(query).await
    }

    /// The search, page by page (100 each), until there is no full page any more or
    /// [`MAX_PRS`] is reached.
    async fn search_with(&self, q: &str) -> Result<Vec<(String, u32)>, Error> {
        let per_page = MAX_PRS.min(100);
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let res: SearchResult = self
                .read_json(self.get("/search/issues").query(&[
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
        self.read_json(self.get(&format!("/repos/{repo}/pulls/{nr}")))
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
        self.read_json(self.get(&format!("/repos/{repo}/commits/{sha}")))
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

    /// Verdicts of the reviewers.
    pub async fn reviews(&self, repo: &str, nr: u32) -> Result<Vec<PullReview>, Error> {
        self.pages(&format!("/repos/{repo}/pulls/{nr}/reviews"))
            .await
    }

    /// Inbox entries: first review requests, then own pull requests (without duplicates).
    /// For each pull request itself and the check status are loaded afterwards.
    pub async fn load_inbox(&self) -> Result<Vec<Item>, Error> {
        let mut items: Vec<Item> = Vec::new();
        let mut seen: Vec<(String, u32)> = Vec::new();
        for relation in [Relation::ReviewRequested, Relation::Own] {
            for (repo, nr) in self.search(relation).await? {
                if seen.contains(&(repo.clone(), nr)) {
                    continue;
                }
                let pull = self.pull(&repo, nr).await?;
                let checks = self.checks(&repo, &pull.head.sha).await?;
                items.push(map::pull_to_item(&repo, &pull, checks, relation));
                seen.push((repo, nr));
            }
        }
        Ok(items)
    }

    /// Review threads and review texts of a pull request. Only reads.
    pub async fn review_input(&self, repo: &str, nr: u32) -> Result<ReviewInput, Error> {
        let lines = self.line_comments(repo, nr).await?;
        let reviews = self.reviews(repo, nr).await?;
        Ok(ReviewInput {
            threads: followup::threads(&lines),
            notes: followup::notes(&reviews),
        })
    }

    /// The open pull request whose branch is `branch` in the same repo, if there is one.
    pub async fn pull_for_branch(
        &self,
        repo: &str,
        branch: &str,
    ) -> Result<Option<PullSummary>, Error> {
        let owner = repo.split('/').next().unwrap_or_default();
        let list: Vec<PullSummary> = self
            .read_json(self.get(&format!("/repos/{repo}/pulls")).query(&[
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
