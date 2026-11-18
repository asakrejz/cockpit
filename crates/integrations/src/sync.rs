//! Fetching for the sync: gets tickets, pull requests, comments, checks and diffs via
//! the clients and packs them as [`Remote`] for `cockpit_core::sync::merge`.
//!
//! If a source fails, its state in the workspace stays unchanged; the error becomes the
//! [`Problem`] of the source. The fetch itself writes nothing.

use cockpit_core::{Loaded, Outcome, PrKey, PrLink, Remote, Review, Source, SourceData, Workspace};
use cockpit_i18n::msg;

use crate::Error;
use crate::github::dto::Pull;
use crate::github::{
    GitHubClient, PrRelation, changes_requested, check_runs_to_core, comments_with_reviews,
    pull_to_item_for, pull_to_review,
};
use crate::jira::{self, JiraClient};
use crate::outbound::{Outbound, Targets};

pub struct GitHubService {
    pub client: GitHubClient,
    /// `owner/name`
    pub repos: Vec<String>,
}

pub struct JiraService {
    pub client: JiraClient,
    pub jql: String,
    /// The query fetches all tickets of a project ([`jira::jql_for_project`]): whatever is neither
    /// assigned nor watched then shows up with the reason „Im Projekt“.
    pub in_project: bool,
}

/// The configured services. A source that is missing is off.
#[derive(Default)]
pub struct Services {
    pub github: Option<GitHubService>,
    pub jira: Option<JiraService>,
}

impl Services {
    pub fn is_empty(&self) -> bool {
        self.github.is_none() && self.jira.is_none()
    }

    pub fn targets(&self) -> Targets {
        Targets {
            github: self.github.is_some(),
            jira: self.jira.is_some(),
            repos: self
                .github
                .as_ref()
                .map(|g| g.repos.clone())
                .unwrap_or_default(),
            routing: cockpit_core::AgentRepos::default(),
        }
    }

    /// Runs the call at the responsible service.
    pub async fn execute(&self, a: &Outbound) -> Result<(), Error> {
        let jira = matches!(
            a,
            Outbound::JiraComment { .. } | Outbound::JiraStatus { .. }
        );
        if jira {
            match &self.jira {
                Some(j) => j.client.execute(a).await,
                None => Err(Error::Coded(msg!(IntegrationsJiraNotSetUp))),
            }
        } else {
            match &self.github {
                Some(g) => g.client.execute(a).await,
                None => Err(Error::Coded(msg!(IntegrationsGithubNotSetUp))),
            }
        }
    }

    /// Queries all sources that are not in `paused`. `known` is the state of the
    /// workspace (for diffs that have not changed), `now_ms` the time of day.
    pub async fn fetch(&self, known: &Workspace, now_ms: u64, paused: &[Source]) -> Remote {
        let now_s = (now_ms / 1000) as i64;
        let mut sources = Vec::new();
        if let Some(g) = self
            .github
            .as_ref()
            .filter(|_| !paused.contains(&Source::GitHub))
        {
            let res = github_load(&g.client, &g.repos, known, now_s).await;
            sources.push(as_source(Source::GitHub, res, now_ms));
        }
        if let Some(j) = self
            .jira
            .as_ref()
            .filter(|_| !paused.contains(&Source::Jira))
        {
            let res = jira_load(&j.client, &j.jql, j.in_project, now_s).await;
            sources.push(as_source(Source::Jira, res, now_ms));
        }
        Remote { sources }
    }
}

fn as_source(source: Source, res: Result<Loaded, Error>, now_ms: u64) -> SourceData {
    SourceData {
        source,
        outcome: match res {
            Ok(loaded) => Outcome::Loaded(loaded),
            Err(e) => Outcome::Failed(e.problem(now_ms)),
        },
    }
}

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
/// vanish from the inbox because of a short outage.
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
    now_s: i64,
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
            Err(e) if only_hint(&e) => out.warn(msg!(
                IntegrationsSyncRepoHint,
                repo = repo,
                error = e.to_string()
            )),
            Err(e) => {
                out.warn(msg!(
                    IntegrationsSyncRepoStale,
                    repo = repo,
                    error = e.to_string()
                ));
                carry_over(known, repo, |_| true, &mut out, &mut taken);
            }
        }
    }
    // … mine first (stable: the order of the service stays within each group) …
    pulls.sort_by_key(|(_, _, r)| !r.mine());

    // … then load the details.
    for (repo, pull, relation) in pulls {
        let nr = pull.number;
        if taken.iter().any(|(r, n)| *r == repo && *n == nr) {
            continue;
        }
        taken.push((repo.clone(), nr));
        match pull_load(client, &repo, &pull, relation, &me, known, now_s).await {
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
                    error = e.to_string()
                ));
                taken.retain(|(r, n)| !(*r == repo && *n == nr));
                carry_over(known, &repo, |n| n == nr, &mut out, &mut taken);
            }
        }
    }
    Ok(out)
}

async fn pull_load(
    client: &GitHubClient,
    repo: &str,
    pull: &Pull,
    relation: PrRelation,
    me: &str,
    known: &Workspace,
    now_s: i64,
) -> Result<(cockpit_core::Item, Review, PrLink), Error> {
    let nr = pull.number;
    let runs = client.check_runs(repo, &pull.head.sha).await?;
    let checks = crate::github::aggregate_checks(&runs);
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

    let mut item = pull_to_item_for(repo, pull, checks, relation, now_s);
    item.comments = comments_with_reviews(&comments, &lines, &verdicts, me, now_s);
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

/// Loads the tickets for `jql`. The default queries ask for the active sprints; if the site or
/// project has none (Jira rejects `openSprints()`, or there is no active sprint), the same query
/// with all open tickets runs instead and the state of the source says so. A JQL set by hand is
/// never rewritten and its errors stay errors.
pub async fn jira_load(
    client: &JiraClient,
    jql: &str,
    in_project: bool,
    now_s: i64,
) -> Result<Loaded, Error> {
    let me = client.me().await?;
    let mut out = Loaded::default();
    // The sprint field is optional: without it (or without the right to list fields) the
    // tickets just carry no sprint.
    let sprint_field = match client.sprint_field().await {
        Ok(f) => f,
        Err(e @ (Error::RateLimit { .. } | Error::NotSignedIn)) => return Err(e),
        Err(e) => {
            out.warn(msg!(
                IntegrationsSyncSprintFieldFailed,
                error = e.to_string()
            ));
            None
        }
    };
    let field = sprint_field.as_deref();
    let fallback = jira::open_tickets_jql(jql);
    let mut used_fallback = false;
    let issues = match (client.search_with_sprint(jql, field).await, &fallback) {
        // No active sprint at all: show what is open instead.
        (Ok(list), Some(open)) if list.is_empty() => {
            let all = client.search_with_sprint(open, field).await?;
            if !all.is_empty() {
                used_fallback = true;
                let any_sprint = all.iter().any(|i| {
                    field.is_some_and(|f| {
                        i.fields
                            .extra
                            .get(f)
                            .is_some_and(|v| jira::sprint_of(v).is_some())
                    })
                });
                out.warn(if any_sprint {
                    msg!(IntegrationsSyncNoActiveSprint)
                } else {
                    msg!(IntegrationsSyncNoSprints)
                });
            }
            all
        }
        (Ok(list), _) => list,
        // Jira does not know `openSprints()` here (e.g. a project without Jira Software).
        (Err(Error::Rejected(m)), Some(open))
            if m.is_empty() || m.to_lowercase().contains("sprint") =>
        {
            used_fallback = true;
            out.warn(msg!(IntegrationsSyncNoSprints));
            client.search_with_sprint(open, field).await?
        }
        (Err(e), _) => return Err(e),
    };
    // Whoever the ticket is not assigned to is shown because of the query, not because of a watch.
    let by_query = if in_project || used_fallback {
        Some("Im Projekt")
    } else if jira::is_sprint_query(jql) {
        Some("Im Sprint")
    } else {
        None
    };
    for issue in issues {
        let mut item = jira::issue_to_item_with(&issue, &me, now_s, field);
        if let Some(text) = by_query
            && item.scope != cockpit_core::Scope::Mine
        {
            item.reason_text = text.into();
        }
        match client.comments(&issue.key).await {
            Ok(k) => item.comments = jira::comments_to_core(&k, &me, now_s),
            Err(Error::NotFound) => continue,
            Err(e) => return Err(e),
        }
        out.items.push(item);
    }
    Ok(out)
}
