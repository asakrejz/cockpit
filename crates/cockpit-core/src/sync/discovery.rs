//! Finding pull requests without a list of repos: the search queries and how their results are
//! merged. Pure; the calls are made in `cockpit-integrations`.
//!
//! GitHub's search answers across every repo the credential can see. Three searches (my own
//! pull requests, review requests, everything I am involved in) are merged without duplicates.
//! Repos on the "watched" list are loaded completely anyway, so their hits are left out here.

/// The search queries, in the order their results are kept.
pub const QUERIES: [&str; 3] = [
    "is:pr is:open author:@me",
    "is:pr is:open review-requested:@me",
    "is:pr is:open involves:@me",
];

/// The merged search results.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// `(owner/name, number)`, in order, without duplicates.
    pub hits: Vec<(String, u32)>,
    /// Repos with more hits than `per_repo` (the surplus is dropped).
    pub cut_repos: Vec<String>,
    /// Hits dropped because `room` was used up.
    pub cut_total: usize,
}

/// Merges the results of [`QUERIES`] (in that order).
///
/// * A hit twice (own *and* involved) counts once.
/// * Hits in `watched` repos are skipped (compared without regard to case).
/// * At most `per_repo` hits per repo and `room` hits in all; what exceeds is reported.
///
/// ```
/// use cockpit_core::sync::discovery::merge_hits;
/// let found = merge_hits(
///     vec![
///         vec![("acme/app".into(), 1), ("acme/web".into(), 2)],
///         vec![("acme/app".into(), 1), ("acme/api".into(), 3)],
///     ],
///     &["acme/web".to_string()],
///     10,
///     10,
/// );
/// assert_eq!(found.hits, [("acme/app".to_string(), 1), ("acme/api".to_string(), 3)]);
/// ```
pub fn merge_hits(
    lists: Vec<Vec<(String, u32)>>,
    watched: &[String],
    per_repo: usize,
    room: usize,
) -> Found {
    let mut found = Found::default();
    for (repo, nr) in lists.into_iter().flatten() {
        if watched.iter().any(|w| w.eq_ignore_ascii_case(&repo))
            || found
                .hits
                .iter()
                .any(|(r, n)| *n == nr && r.eq_ignore_ascii_case(&repo))
        {
            continue;
        }
        let in_repo = found
            .hits
            .iter()
            .filter(|(r, _)| r.eq_ignore_ascii_case(&repo))
            .count();
        if in_repo >= per_repo {
            if !found.cut_repos.contains(&repo) {
                found.cut_repos.push(repo);
            }
            continue;
        }
        if found.hits.len() >= room {
            found.cut_total += 1;
            continue;
        }
        found.hits.push((repo, nr));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(r: &str, n: u32) -> (String, u32) {
        (r.to_string(), n)
    }

    #[test]
    fn queries_cover_own_requested_and_involved() {
        assert!(QUERIES.iter().all(|q| q.starts_with("is:pr is:open ")));
        assert!(QUERIES[0].ends_with("author:@me"));
        assert!(QUERIES[1].ends_with("review-requested:@me"));
        assert!(QUERIES[2].ends_with("involves:@me"));
    }

    #[test]
    fn duplicates_across_queries_count_once_and_the_first_order_stays() {
        let f = merge_hits(
            vec![
                vec![hit("a/x", 1)],
                vec![hit("b/y", 2), hit("a/x", 1)],
                vec![hit("A/X", 1), hit("a/x", 3)],
            ],
            &[],
            10,
            10,
        );
        assert_eq!(f.hits, [hit("a/x", 1), hit("b/y", 2), hit("a/x", 3)]);
    }

    #[test]
    fn the_same_number_in_two_repos_is_two_pull_requests() {
        let f = merge_hits(vec![vec![hit("a/x", 7), hit("b/y", 7)]], &[], 10, 10);
        assert_eq!(f.hits.len(), 2);
    }

    #[test]
    fn watched_repos_are_left_to_the_full_load() {
        let f = merge_hits(
            vec![vec![hit("Acme/App", 1), hit("acme/web", 2)]],
            &["acme/app".to_string()],
            10,
            10,
        );
        assert_eq!(f.hits, [hit("acme/web", 2)]);
    }

    #[test]
    fn limits_per_repo_and_in_total_are_reported() {
        let f = merge_hits(
            vec![vec![
                hit("a/x", 1),
                hit("a/x", 2),
                hit("a/x", 3),
                hit("b/y", 4),
                hit("c/z", 5),
            ]],
            &[],
            2,
            3,
        );
        assert_eq!(f.hits, [hit("a/x", 1), hit("a/x", 2), hit("b/y", 4)]);
        assert_eq!(f.cut_repos, ["a/x"]);
        assert_eq!(f.cut_total, 1);
    }

    #[test]
    fn no_room_loads_nothing() {
        let f = merge_hits(vec![vec![hit("a/x", 1)]], &[], 10, 0);
        assert!(f.hits.is_empty());
        assert_eq!(f.cut_total, 1);
    }
}
