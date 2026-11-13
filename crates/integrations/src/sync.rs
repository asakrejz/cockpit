//! Fetching for the sync: gets tickets, pull requests, comments, checks and diffs via
//! the clients and packs them as [`Remote`] for `cockpit_core::sync::merge`.
//!
//! If a source fails, its state in the workspace stays unchanged; the error becomes the
//! [`Problem`] of the source. The fetch itself writes nothing.

use cockpit_core::{
    Loaded, Outcome, PrKey, PrLink, Remote, Review, Source, SourceData, Workspace,
};

use crate::Error;
use crate::github::{
    GitHubClient, Relation, changes_requested, check_runs_to_core, comments_with_reviews,
    pull_to_item, pull_to_review,
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
                None => Err(Error::Config("Jira ist nicht eingerichtet".into())),
            }
        } else {
            match &self.github {
                Some(g) => g.client.execute(a).await,
                None => Err(Error::Config("GitHub ist nicht eingerichtet".into())),
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

pub async fn github_load(
    client: &GitHubClient,
    repos: &[String],
    known: &Workspace,
    now_s: i64,
) -> Result<Loaded, Error> {
    let me = client.me().await?;
    let mut out = Loaded::default();
    let mut seen: Vec<(String, u32)> = Vec::new();

    for repo in repos {
        let mut hits: Vec<(u32, Relation)> = Vec::new();
        for relation in [Relation::ReviewRequested, Relation::Own] {
            match client.search_in(relation, repo).await {
                Ok(list) => hits.extend(list.into_iter().map(|(_, nr)| (nr, relation))),
                Err(e) if only_hint(&e) => {
                    out.warnings.push(format!("{repo}: {e}"));
                    hits.clear();
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        for (nr, relation) in hits {
            if seen.iter().any(|(r, n)| r == repo && *n == nr) {
                continue;
            }
            seen.push((repo.clone(), nr));
            match pull_load(client, repo, nr, relation, &me, known, now_s).await {
                Ok((item, review, link)) => {
                    out.items.push(item);
                    out.reviews.push(review);
                    out.prs.push(link);
                }
                // Closed or deleted between search and fetch.
                Err(Error::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(out)
}

async fn pull_load(
    client: &GitHubClient,
    repo: &str,
    nr: u32,
    relation: Relation,
    me: &str,
    known: &Workspace,
    now_s: i64,
) -> Result<(cockpit_core::Item, Review, PrLink), Error> {
    let pull = client.pull(repo, nr).await?;
    let runs = client.check_runs(repo, &pull.head.sha).await?;
    let checks = crate::github::aggregate_checks(&runs);
    let verdicts = client.reviews(repo, nr).await?;
    let comments = client.comments(repo, nr).await?;
    let lines = client.line_comments(repo, nr).await?;

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

    let mut item = pull_to_item(repo, &pull, checks, relation, now_s);
    item.comments = comments_with_reviews(&comments, &lines, &verdicts, me, now_s);
    if relation == Relation::Own
        && let Some(pr) = item.pr.as_mut()
    {
        pr.changes_requested = changes_requested(&verdicts, me);
    }
    let mut review = pull_to_review(&pull, &files, checks);
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

pub async fn jira_load(
    client: &JiraClient,
    jql: &str,
    in_project: bool,
    now_s: i64,
) -> Result<Loaded, Error> {
    let me = client.me().await?;
    let mut out = Loaded::default();
    for issue in client.search(jql).await? {
        let mut item = jira::issue_to_item(&issue, &me, now_s);
        if in_project && item.scope != cockpit_core::Scope::Mine {
            item.reason_text = "Im Projekt".into();
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
