//! Loading pull requests, check runs and diffs from the repos, for the sync.

use cockpit_core::sync::discovery;
use cockpit_core::{Loaded, PrKey, PrLink, Review, Workspace};
use cockpit_i18n::msg;

use crate::Error;
use crate::github::dto::Pull;
use crate::github::{
    GitHubClient, PrRelation, changes_requested, check_runs_to_core, comments_with_reviews,
    pull_to_item_for, pull_to_review,
};

/// A missing or forbidden repo is a hint, not an outage of the whole source.
fn only_hint(e: &Error) -> bool {
    matches!(
        e,
        Error::NotFound | Error::NoPermission | Error::Rejected(_)
    )
}

/// An error that holds for every repo: the source fails as a whole (and keeps its data).
fn all_repos(e: &Error) -> bool {
    matches!(e, Error::RateLimit { .. } | Error::NotSignedIn)
}

/// Copies what the workspace knows about the pull requests of `repo` that `wanted` accepts into
/// the result – for a repo or pull request that could not be read this time, so that it does not
/// vanish because of a short outage.
fn carry_over(
    known: &Workspace,
    repo: &str,
    wanted: impl Fn(u32) -> bool,
    out: &mut Loaded,
    taken: &mut Vec<(String, u32)>,
) {
    for link in known.sync.prs.iter().filter(|l| l.repo == repo) {
        if !wanted(link.number) || taken.iter().any(|(r, n)| r == repo && *n == link.number) {
            continue;
        }
        let key = link.key();
        let id = crate::text::id_from(&["gh", repo, &link.number.to_string()]);
        let (Some(item), Some(review)) = (known.item(&id), known.review(&key)) else {
            continue;
        };
        out.items.push(item.clone());
        out.reviews.push(review.clone());
        out.prs.push(link.clone());
        taken.push((repo.to_string(), link.number));
    }
}

/// All open pull requests of the repos (not only mine): per repo the list of open pull requests,
/// at most [`PrLimits`] per repo and in total. The relation to the user comes from the pull
/// request itself (author, requested reviewers, assignees). Pull requests of colleagues get
/// the scope `Team` and only the cheap data (checks, diff); the comments and verdicts are loaded
/// for pull requests that concern me.
pub async fn github_load(
    client: &GitHubClient,
    repos: &[String],
    known: &Workspace,
) -> Result<Loaded, Error> {
    let me = client.me().await?;
    let limits = client.limits();
    let mut out = Loaded::default();
    let mut taken: Vec<(String, u32)> = Vec::new();

    // First collect the lists (a failed repo does not block the others) …
    let mut pulls: Vec<(String, Pull, PrRelation)> = Vec::new();
    let mut left = limits.total;
    for repo in repos {
        if left == 0 {
            out.warn(msg!(
                IntegrationsSyncNotLoaded,
                repo = repo,
                n = limits.total
            ));
            continue;
        }
        let max = limits.per_repo.min(left);
        match client.pulls_open(repo, max).await {
            Ok(page) => {
                if page.truncated {
                    out.warn(if max < limits.per_repo {
                        msg!(
                            IntegrationsSyncTruncatedTotal,
                            repo = repo,
                            n = limits.total
                        )
                    } else {
                        msg!(
                            IntegrationsSyncTruncatedPerRepo,
                            repo = repo,
                            n = limits.per_repo
                        )
                    });
                }
                left -= page.pulls.len();
                for pull in page.pulls {
                    let relation = PrRelation::of(&pull, &me);
                    pulls.push((repo.clone(), pull, relation));
                }
            }
            Err(e) if all_repos(&e) => return Err(e),
            Err(e) if only_hint(&e) => {
                out.warn(msg!(IntegrationsSyncRepoHint, repo = repo, error = e.msg()))
            }
            Err(e) => {
                out.warn(msg!(
                    IntegrationsSyncRepoStale,
                    repo = repo,
                    error = e.msg()
                ));
                carry_over(known, repo, |_| true, &mut out, &mut taken);
            }
        }
    }
    // … then the pull requests the search finds in all other repos the key can see …
    let hits = discover(
        client,
        repos,
        left,
        limits.per_repo,
        known,
        &mut out,
        &mut taken,
    )
    .await?;
    for (repo, nr) in hits {
        match client.pull(&repo, nr).await {
            Ok(pull) => {
                let relation = PrRelation::of(&pull, &me);
                pulls.push((repo, pull, relation));
            }
            // Closed or no longer visible since the search.
            Err(Error::NotFound | Error::NoPermission) => {}
            Err(e) if all_repos(&e) => return Err(e),
            Err(e) => {
                out.warn(msg!(
                    IntegrationsSyncPrStale,
                    repo = &repo,
                    number = nr,
                    error = e.msg()
                ));
                carry_over(known, &repo, |n| n == nr, &mut out, &mut taken);
            }
        }
    }
    // … mine first (stable: the order of the service stays within each group) …
    pulls.sort_by_key(|(_, _, r)| !r.mine());

    for (repo, pull, relation) in pulls {
        let nr = pull.number;
        if taken.iter().any(|(r, n)| *r == repo && *n == nr) {
            continue;
        }
        taken.push((repo.clone(), nr));
        match pull_load(client, &repo, &pull, relation, &me, known).await {
            Ok((item, review, link)) => {
                out.items.push(item);
                out.reviews.push(review);
                out.prs.push(link);
            }
            // Closed or deleted since the list.
            Err(Error::NotFound) => {}
            Err(e) if all_repos(&e) => return Err(e),
            Err(e) => {
                out.warn(msg!(
                    IntegrationsSyncPrStale,
                    repo = &repo,
                    number = nr,
                    error = e.msg()
                ));
                taken.retain(|(r, n)| !(*r == repo && *n == nr));
                carry_over(known, &repo, |n| n == nr, &mut out, &mut taken);
            }
        }
    }
    Ok(out)
}

/// The pull requests of repos that are not watched: three searches (see
/// [`cockpit_core::sync::discovery`]), merged without duplicates and bounded by `room` (what the
/// watched repos left of the total) and `per_repo`. A failed search is a hint, not an outage:
/// what the workspace knew about those repos stays, so nothing vanishes for one bad minute.
async fn discover(
    client: &GitHubClient,
    watched: &[String],
    room: usize,
    per_repo: usize,
    known: &Workspace,
    out: &mut Loaded,
    taken: &mut Vec<(String, u32)>,
) -> Result<Vec<(String, u32)>, Error> {
    let mut lists = Vec::new();
    let mut failed = false;
    for query in discovery::QUERIES {
        match client.search_pulls(query).await {
            Ok(list) => lists.push(list),
            Err(Error::NotSignedIn) => return Err(Error::NotSignedIn),
            Err(e) => {
                // A search limit is separate from the normal one: keep going without it.
                failed = true;
                out.warn(msg!(IntegrationsSyncSearchFailed, error = e.msg()));
                break;
            }
        }
    }
    if failed {
        let mut others: Vec<&str> = Vec::new();
        for link in &known.sync.prs {
            if !watched.iter().any(|w| w.eq_ignore_ascii_case(&link.repo))
                && !others.contains(&link.repo.as_str())
            {
                others.push(&link.repo);
            }
        }
        for repo in others {
            carry_over(known, repo, |_| true, out, taken);
        }
    }
    let found = discovery::merge_hits(lists, watched, per_repo, room);
    for repo in &found.cut_repos {
        out.warn(msg!(
            IntegrationsSyncTruncatedPerRepo,
            repo = repo,
            n = per_repo
        ));
    }
    if found.cut_total > 0 {
        out.warn(msg!(IntegrationsSyncSearchCut, n = found.cut_total));
    }
    Ok(found.hits)
}

async fn pull_load(
    client: &GitHubClient,
    repo: &str,
    pull: &Pull,
    relation: PrRelation,
    me: &str,
    known: &Workspace,
) -> Result<(cockpit_core::Item, Review, PrLink), Error> {
    let nr = pull.number;
    // Without the permission "Checks" (fine-grained token) only the commit status is read, or the
    // state stays unknown; the pull request loads either way.
    let crate::github::LoadedChecks { runs, checks } =
        client.checks_load(repo, &pull.head.sha).await?;
    // Comments and verdicts only for pull requests that concern me.
    let (verdicts, comments, lines) = if relation.mine() {
        (
            client.reviews(repo, nr).await?,
            client.comments(repo, nr).await?,
            client.line_comments(repo, nr).await?,
        )
    } else {
        Default::default()
    };

    // The diff only changes with a new commit.
    let key = PrKey::new(repo, nr);
    let unchanged = known
        .sync
        .pr(&key)
        .filter(|l| l.repo == repo && l.head_sha == pull.head.sha)
        .and_then(|_| known.review(&key))
        .filter(|r| r.repo == repo || r.repo.is_empty());
    let files = match unchanged {
        Some(_) => Vec::new(),
        None => client.files(repo, nr).await?,
    };

    let mut item = pull_to_item_for(repo, pull, checks, relation);
    item.comments = comments_with_reviews(&comments, &lines, &verdicts, me);
    if relation.author
        && let Some(pr) = item.pr.as_mut()
    {
        pr.changes_requested = changes_requested(&verdicts, me);
    }
    let mut review = pull_to_review(pull, &files, checks);
    review.repo = repo.to_string();
    if let Some(old) = unchanged {
        review.files = old.files.clone();
    }
    review.check_runs = check_runs_to_core(&runs);
    let link = PrLink {
        number: nr,
        repo: repo.to_string(),
        head_sha: pull.head.sha.clone(),
    };
    Ok((item, review, link))
}
