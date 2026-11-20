//! The filter of the Reviews tab: which pull requests the list shows. Display only, nothing is
//! written to GitHub. The choice is part of the workspace ([`Workspace::review_filter`]) and
//! changes by [`Action::SetReviewFilter`](crate::Action).
//!
//! Nothing here limits what the sync loads: it finds every pull request that concerns me in
//! every repo the credential can see (see [`crate::sync::discovery`]). The filter only narrows
//! the list.

use serde::{Deserialize, Serialize};

use crate::filters::Ownership;
use crate::model::PrKey;
use crate::review::Review;
use crate::workspace::Workspace;

/// What the Reviews list shows. An empty list of repos or projects means „all“.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ReviewFilter {
    /// Whose pull requests (mine / all).
    #[serde(default)]
    pub ownership: Ownership,
    /// Only pull requests of these repos (`owner/name`). Empty = every repo.
    #[serde(default)]
    pub repos: Vec<String>,
    /// Only pull requests linked to an item of one of these GitHub projects (node IDs). Empty =
    /// no restriction.
    #[serde(default)]
    pub projects: Vec<String>,
}

impl ReviewFilter {
    /// Nothing is narrowed except possibly whose pull requests.
    pub fn is_default(&self) -> bool {
        *self == ReviewFilter::default()
    }

    /// Is a repo or project chosen (not counting the ownership)?
    pub fn narrows(&self) -> bool {
        !self.repos.is_empty() || !self.projects.is_empty()
    }

    /// Is `repo` shown? Case does not matter.
    pub fn allows_repo(&self, repo: &str) -> bool {
        self.repos.is_empty() || self.repos.iter().any(|r| r.eq_ignore_ascii_case(repo))
    }

    /// Adds or removes a repo from the choice.
    pub fn toggle_repo(&mut self, repo: &str) {
        match self.repos.iter().position(|r| r.eq_ignore_ascii_case(repo)) {
            Some(i) => {
                self.repos.remove(i);
            }
            None => self.repos.push(repo.to_string()),
        }
    }

    /// Adds or removes a project (node ID) from the choice.
    pub fn toggle_project(&mut self, id: &str) {
        match self.projects.iter().position(|p| p == id) {
            Some(i) => {
                self.projects.remove(i);
            }
            None => self.projects.push(id.to_string()),
        }
    }
}

/// The repos of one owner (user or organization) with their number of pull requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoGroup {
    pub owner: String,
    /// `(owner/name, number of pull requests)`, sorted by name.
    pub repos: Vec<(String, usize)>,
}

impl RepoGroup {
    /// All pull requests of the group.
    pub fn count(&self) -> usize {
        self.repos.iter().map(|(_, n)| n).sum()
    }
}

/// A GitHub project as a filter choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectChoice {
    /// Node ID.
    pub id: String,
    pub title: String,
    /// Pull requests linked to an item of the project.
    pub count: usize,
}

/// Groups repos (with duplicates = one pull request each) by owner, sorted without regard to
/// case. `query` keeps only repos whose name contains every word of it.
///
/// ```
/// use cockpit_core::review_filter::group_repos;
/// let g = group_repos(["acme/web", "acme/app", "acme/app", "zeta/x"].into_iter(), "");
/// assert_eq!(g[0].owner, "acme");
/// assert_eq!(g[0].repos, [("acme/app".to_string(), 2), ("acme/web".to_string(), 1)]);
/// assert_eq!(group_repos(["acme/web", "zeta/x"].into_iter(), "zeta")[0].owner, "zeta");
/// ```
pub fn group_repos<'a>(repos: impl Iterator<Item = &'a str>, query: &str) -> Vec<RepoGroup> {
    let mut counted: Vec<(String, usize)> = Vec::new();
    for repo in repos.filter(|r| !r.is_empty()) {
        match counted
            .iter_mut()
            .find(|(r, _)| r.eq_ignore_ascii_case(repo))
        {
            Some(c) => c.1 += 1,
            None => counted.push((repo.to_string(), 1)),
        }
    }
    counted.retain(|(r, _)| crate::setup::repo_matches(r, query));
    counted.sort_by_key(|(r, _)| r.to_lowercase());
    let mut groups: Vec<RepoGroup> = Vec::new();
    for (repo, n) in counted {
        let owner = repo.split('/').next().unwrap_or_default().to_string();
        match groups
            .iter_mut()
            .find(|g| g.owner.eq_ignore_ascii_case(&owner))
        {
            Some(g) => g.repos.push((repo, n)),
            None => groups.push(RepoGroup {
                owner,
                repos: vec![(repo, n)],
            }),
        }
    }
    groups
}

impl Workspace {
    /// The pull requests the Reviews list shows: `filter` applied to all reviews. `keep` stays
    /// in any case (the review that is open right now).
    pub fn reviews_filtered(&self, filter: &ReviewFilter, keep: Option<&PrKey>) -> Vec<&Review> {
        let in_projects: Vec<PrKey> = if filter.projects.is_empty() {
            Vec::new()
        } else {
            self.project_reviews(&filter.projects)
        };
        self.reviews_for(filter.ownership, keep)
            .into_iter()
            .filter(|r| {
                keep.is_some_and(|k| k.same(&r.key()))
                    || (filter.allows_repo(&r.repo)
                        && (filter.projects.is_empty()
                            || in_projects.iter().any(|k| k.same(&r.key()))))
            })
            .collect()
    }

    /// The reviews linked to an item of one of the projects (`ids` are node IDs).
    pub fn project_reviews(&self, ids: &[String]) -> Vec<PrKey> {
        let mut out: Vec<PrKey> = Vec::new();
        for item in self.items.iter().filter(|i| {
            i.project
                .as_ref()
                .is_some_and(|p| ids.contains(&p.project_id))
        }) {
            for r in self.linked_reviews(&item.id) {
                if !out.iter().any(|k| k.same(&r.key())) {
                    out.push(r.key());
                }
            }
        }
        out
    }

    /// The repos that have pull requests in the Reviews list (before any repo or project choice,
    /// with the chosen ownership), grouped by owner.
    pub fn review_repo_groups(&self, ownership: Ownership, query: &str) -> Vec<RepoGroup> {
        group_repos(
            self.reviews_for(ownership, None)
                .into_iter()
                .map(|r| r.repo.as_str()),
            query,
        )
    }

    /// The GitHub projects with pull requests linked to them; empty when no project is
    /// connected or none has a linked pull request.
    pub fn review_projects(&self) -> Vec<ProjectChoice> {
        let mut out: Vec<ProjectChoice> = self
            .sync
            .projects
            .iter()
            .map(|p| ProjectChoice {
                id: p.id.clone(),
                title: if p.title.is_empty() {
                    format!("{}/{}", p.owner, p.number)
                } else {
                    p.title.clone()
                },
                count: self.project_reviews(std::slice::from_ref(&p.id)).len(),
            })
            .collect();
        out.sort_by_key(|p| p.title.to_lowercase());
        out
    }

    /// Replaces the chosen Reviews filter. `false` if nothing changes.
    pub fn set_review_filter(&mut self, filter: ReviewFilter) -> bool {
        if self.review_filter == filter {
            return false;
        }
        self.review_filter = filter;
        true
    }
}

#[cfg(test)]
mod tests;
