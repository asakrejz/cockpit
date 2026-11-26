//! The filter of the review list: which pull requests it shows. The choice is part of the
//! workspace ([`Workspace::review_filter`]) and changes by [`Action::SetReviewFilter`](crate::Action).
//!
//! Nothing here limits what the sync loads; the filter only narrows the list.

use serde::{Deserialize, Serialize};

use crate::model::PrKey;
use crate::review::Review;
use crate::workspace::Workspace;

/// Whose pull requests the list shows: "Mine" keeps only those that concern me, „Alle“ (the
/// default) lists all open pull requests of the repos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ownership {
    Mine,
    #[default]
    All,
}

impl Ownership {
    /// The IDs, in display order.
    pub const IDS: &'static [&'static str] = &["mine", "all"];

    pub fn id(self) -> &'static str {
        match self {
            Ownership::Mine => "mine",
            Ownership::All => "all",
        }
    }

    pub fn from_id(id: &str) -> Option<Ownership> {
        match id {
            "mine" => Some(Ownership::Mine),
            "all" => Some(Ownership::All),
            _ => None,
        }
    }
}

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

/// With at least this many repos in the list the sections are grouped under their owner first.
pub const OWNER_GROUPS_FROM: usize = 8;

/// The pull requests of one repo in the Reviews list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSection {
    /// `owner/name`; empty = unknown.
    pub repo: String,
    /// In the order of the list.
    pub keys: Vec<PrKey>,
    /// How many of them need my attention (see [`Workspace::review_needs_attention`]).
    pub attention: usize,
}

impl ReviewSection {
    pub fn owner(&self) -> &str {
        self.repo.split('/').next().unwrap_or_default()
    }
}

/// Sections under an owner header; `owner` is `None` when the list is not grouped by owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewGroup {
    pub owner: Option<String>,
    pub sections: Vec<ReviewSection>,
}

impl ReviewGroup {
    pub fn count(&self) -> usize {
        self.sections.iter().map(|s| s.keys.len()).sum()
    }

    pub fn attention(&self) -> usize {
        self.sections.iter().map(|s| s.attention).sum()
    }
}

/// Sorts sections: most pull requests that need my attention first, then by name.
fn sort_sections(sections: &mut [ReviewSection]) {
    sections.sort_by(|a, b| {
        b.attention
            .cmp(&a.attention)
            .then_with(|| a.repo.to_lowercase().cmp(&b.repo.to_lowercase()))
    });
}

/// Groups sections by owner when there are at least `owners_from` of them and more than one
/// owner; otherwise one group without an owner header. Groups are sorted like sections: by the
/// number of pull requests that need attention, then by name.
///
/// ```
/// use cockpit_core::review_filter::{group_sections, ReviewSection};
/// let s = |r: &str, a| ReviewSection { repo: r.into(), keys: vec![], attention: a };
/// let groups = group_sections(vec![s("a/x", 0), s("b/y", 2)], 2);
/// assert_eq!(groups[0].owner.as_deref(), Some("b"));
/// assert_eq!(group_sections(vec![s("a/x", 0), s("b/y", 2)], 3).len(), 1);
/// ```
pub fn group_sections(sections: Vec<ReviewSection>, owners_from: usize) -> Vec<ReviewGroup> {
    let owners = {
        let mut o: Vec<String> = sections.iter().map(|s| s.owner().to_lowercase()).collect();
        o.sort();
        o.dedup();
        o.len()
    };
    if sections.len() < owners_from || owners < 2 {
        return vec![ReviewGroup {
            owner: None,
            sections,
        }];
    }
    let mut groups: Vec<ReviewGroup> = Vec::new();
    for section in sections {
        let owner = section.owner().to_string();
        match groups.iter_mut().find(|g| {
            g.owner
                .as_deref()
                .is_some_and(|o| o.eq_ignore_ascii_case(&owner))
        }) {
            Some(g) => g.sections.push(section),
            None => groups.push(ReviewGroup {
                owner: Some(owner),
                sections: vec![section],
            }),
        }
    }
    groups.sort_by(|a, b| {
        b.attention().cmp(&a.attention()).then_with(|| {
            a.owner
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .cmp(&b.owner.as_deref().unwrap_or_default().to_lowercase())
        })
    });
    groups
}

/// Is this repo, or its owner, collapsed? `collapsed` holds `owner/name` for a repo and `owner`
/// for an owner header.
pub fn is_collapsed(collapsed: &[String], owner: Option<&str>, repo: &str) -> bool {
    let has = |k: &str| collapsed.iter().any(|c| c.eq_ignore_ascii_case(k));
    has(repo) || owner.is_some_and(has)
}

/// The pull requests that are on screen, in order: collapsed repos and owners contribute none,
/// except the one that is open (`current`), whose section always counts as expanded. Up and
/// down move along this list.
pub fn visible_keys(
    groups: &[ReviewGroup],
    collapsed: &[String],
    current: Option<&PrKey>,
) -> Vec<PrKey> {
    let mut out = Vec::new();
    for g in groups {
        for s in &g.sections {
            let holds_current = current.is_some_and(|c| s.keys.iter().any(|k| k.same(c)));
            if holds_current || !is_collapsed(collapsed, g.owner.as_deref(), &s.repo) {
                out.extend(s.keys.iter().cloned());
            }
        }
    }
    out
}

/// The key before or after `current` in `keys`; the first one when nothing is open.
pub fn step_key(keys: &[PrKey], current: Option<&PrKey>, forward: bool) -> Option<PrKey> {
    let at = current.and_then(|c| keys.iter().position(|k| k.same(c)));
    let next = match (at, forward) {
        (None, _) => 0,
        (Some(i), true) => (i + 1).min(keys.len().saturating_sub(1)),
        (Some(i), false) => i.saturating_sub(1),
    };
    keys.get(next).cloned()
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

    /// Does this pull request need my attention? A review is requested from me, or it is mine
    /// and changes were requested.
    pub fn review_needs_attention(&self, review: &Review) -> bool {
        let key = review.key();
        self.items
            .iter()
            .find_map(|i| i.pr.as_ref().filter(|p| p.key().same(&key)))
            .is_some_and(|p| !p.merged && (p.requested_me || (p.mine && p.changes_requested)))
    }

    /// One section per repo for the given list of reviews, sorted by the number of pull
    /// requests that need my attention, then by name. Inside a section the order of `list`
    /// stays.
    pub fn review_sections(&self, list: &[&Review]) -> Vec<ReviewSection> {
        let mut sections: Vec<ReviewSection> = Vec::new();
        for r in list {
            let attention = usize::from(self.review_needs_attention(r));
            match sections
                .iter_mut()
                .find(|s| s.repo.eq_ignore_ascii_case(&r.repo))
            {
                Some(s) => {
                    s.keys.push(r.key());
                    s.attention += attention;
                }
                None => sections.push(ReviewSection {
                    repo: r.repo.clone(),
                    keys: vec![r.key()],
                    attention,
                }),
            }
        }
        sort_sections(&mut sections);
        sections
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
