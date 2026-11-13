use super::*;

impl Workspace {
    // ---------- Repo identity of old states ----------

    /// Fills the repo wherever an old state lacks it (before several repos were supported):
    /// reviews and pull request references take it from the sync link, from the inbox entry
    /// of the pull request, or – when only one repo is configured – from `only_repo`.
    /// Pull requests whose number is ambiguous without a repo stay as they are, unless
    /// `only_repo` is given. Idempotent. Returns whether anything changed.
    pub fn fill_repos(&mut self, only_repo: Option<&str>) -> bool {
        let mut changed = false;
        let unique = |repos: Vec<String>| -> Option<String> {
            let mut it = repos.into_iter();
            let first = it.next()?;
            it.all(|r| r == first).then_some(first)
        };

        // 1. Reviews: sync link, then the inbox entry of the same number, then the only repo.
        for i in 0..self.reviews.len() {
            if !self.reviews[i].repo.is_empty() {
                continue;
            }
            let number = self.reviews[i].number;
            let from_links = unique(
                self.sync
                    .prs
                    .iter()
                    .filter(|p| p.number == number)
                    .map(|p| p.repo.clone())
                    .collect(),
            );
            let from_items = unique(
                self.items
                    .iter()
                    .filter(|it| it.kind == Kind::PullRequest)
                    .filter(|it| it.pr.as_ref().is_some_and(|p| p.number == number))
                    .filter_map(|it| it.repo.clone().filter(|r| !r.is_empty()))
                    .collect(),
            );
            if let Some(repo) = from_links
                .or(from_items)
                .or_else(|| only_repo.map(str::to_string))
            {
                self.reviews[i].repo = repo;
                changed = true;
            }
        }

        // 2. References on inbox entries: the entry's own repo (pull requests), the review
        //    with that number if unambiguous, then the only repo.
        let review_repos: Vec<(u32, String)> = self
            .reviews
            .iter()
            .filter(|r| !r.repo.is_empty())
            .map(|r| (r.number, r.repo.clone()))
            .collect();
        for item in &mut self.items {
            let own = item.repo.clone().filter(|r| !r.is_empty());
            let is_pr = item.kind == Kind::PullRequest;
            let Some(pr) = item.pr.as_mut().filter(|p| p.repo.is_empty()) else {
                continue;
            };
            let from_reviews = unique(
                review_repos
                    .iter()
                    .filter(|(n, _)| *n == pr.number)
                    .map(|(_, r)| r.clone())
                    .collect(),
            );
            let repo = own
                .filter(|_| is_pr)
                .or(from_reviews)
                .or_else(|| only_repo.map(str::to_string));
            if let Some(repo) = repo {
                pr.repo = repo;
                changed = true;
            }
        }

        // 3. Runs: follow-up jobs work in the repo of their pull request; real runs that were
        //    started before were tied to the placeholder repo and now take the only repo.
        for i in 0..self.runs.len() {
            let run = &self.runs[i];
            let stale = run.repo.is_empty() || (run.real && run.repo == DEMO_REPO);
            if !stale {
                continue;
            }
            let from_review = run
                .kind
                .review()
                .and_then(|k| self.review(&k).map(|r| r.repo.clone()))
                .filter(|r| !r.is_empty());
            if let Some(repo) = from_review.or_else(|| only_repo.map(str::to_string)) {
                self.runs[i].repo = repo;
                changed = true;
            }
        }
        changed
    }
}
