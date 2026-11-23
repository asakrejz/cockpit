//! Fetching for the sync: gets tickets, pull requests, comments, checks and diffs via
//! the clients and packs them as [`Remote`] for `cockpit_core::sync::merge`.
//!
//! If a source fails, its state in the workspace stays unchanged; the error becomes the
//! [`Problem`] of the source. The fetch itself writes nothing.

use cockpit_core::sync::discovery;
use cockpit_core::{Loaded, Outcome, PrKey, PrLink, Remote, Review, Source, SourceData, Workspace};
use cockpit_i18n::msg;

use crate::Error;
use crate::github::dto::Pull;
use crate::github::projects::{self, Heavy, MapCtx};
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
    /// Which fields the detail view hides or puts first.
    pub fields: jira::FieldPrefs,
}

/// GitHub Projects as the source of tickets.
pub struct ProjectsService {
    pub client: GitHubClient,
    /// The projects chosen in the setup.
    pub projects: Vec<cockpit_core::ProjectRef>,
    /// The field that counts as the ticket status (default `Status`).
    pub status_field: Option<String>,
    /// The field that holds the team (default `Team`).
    pub team_field: Option<String>,
}

/// The configured services. A source that is missing is off.
#[derive(Default)]
pub struct Services {
    pub github: Option<GitHubService>,
    pub jira: Option<JiraService>,
    pub projects: Option<ProjectsService>,
}

impl Services {
    pub fn is_empty(&self) -> bool {
        self.github.is_none() && self.jira.is_none() && self.projects.is_none()
    }

    pub fn targets(&self) -> Targets {
        Targets {
            github: self.github.is_some(),
            jira: self.jira.is_some(),
            projects: self.projects.is_some(),
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
        // Changes at GitHub Projects go through the client of the project source.
        if matches!(
            a,
            Outbound::ProjectField { .. } | Outbound::DraftEdit { .. } | Outbound::Issue { .. }
        ) {
            return match (&self.projects, &self.github) {
                (Some(p), _) => p.client.execute(a).await,
                (None, Some(g)) => g.client.execute(a).await,
                (None, None) => Err(Error::Coded(msg!(IntegrationsPlanProjectsOff))),
            };
        }
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
            let res = jira_load(&j.client, &j.jql, j.in_project, &j.fields, now_s).await;
            sources.push(as_source(Source::Jira, res, now_ms));
        }
        if let Some(p) = self
            .projects
            .as_ref()
            .filter(|_| !paused.contains(&Source::GitHubProject))
        {
            let res = projects_load(p, known, now_s).await;
            sources.push(as_source(Source::GitHubProject, res, now_ms));
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
    now_s: i64,
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
    prefs: &jira::FieldPrefs,
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
            out.warn(msg!(IntegrationsSyncSprintFieldFailed, error = e.msg()));
            None
        }
    };
    let field = sprint_field.as_deref();
    // The team field is optional too: without it no ticket has a team.
    let team_field = match client.team_field().await {
        Ok(f) => f,
        Err(e @ (Error::RateLimit { .. } | Error::NotSignedIn)) => return Err(e),
        Err(e) => {
            out.warn(msg!(IntegrationsSyncTeamFieldFailed, error = e.msg()));
            None
        }
    };
    let team = team_field.as_deref();
    let fallback = jira::open_tickets_jql(jql);
    let mut used_fallback = false;
    let page = match (
        client.search_page_fields(jql, &[field, team]).await,
        &fallback,
    ) {
        // No active sprint at all: show what is open instead.
        (Ok(list), Some(open)) if list.issues.is_empty() => {
            let all = client.search_page_fields(open, &[field, team]).await?;
            if !all.issues.is_empty() {
                used_fallback = true;
                let any_sprint = all.issues.iter().any(|i| {
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
            client.search_page_fields(open, &[field, team]).await?
        }
        (Err(e), _) => return Err(e),
    };
    if page.truncated {
        out.warn(msg!(IntegrationsSyncJiraTruncated, n = jira::MAX_ISSUES));
    }
    let issues = page.issues;
    // Whoever the ticket is not assigned to is shown because of the query, not because of a watch.
    let by_query = if in_project || used_fallback {
        Some("Im Projekt")
    } else if jira::is_sprint_query(jql) {
        Some("Im Sprint")
    } else {
        None
    };
    for issue in issues {
        let mut item = jira::issue_to_item_full(&issue, &me, now_s, field, team);
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
        // Fields and attachments come with the search; images in the texts find their file.
        let details = jira::details_of(&issue, &me, field, prefs);
        for doc in item
            .description_rich
            .iter_mut()
            .chain(item.comments.iter_mut().filter_map(|c| c.rich.as_mut()))
        {
            cockpit_core::details::link_media(doc, &details.attachments);
        }
        item.url = client.browse_url(&issue.key);
        item.details = Some(details);
        out.items.push(item);
    }
    Ok(out)
}

/// An error that holds for every project: the source fails as a whole (and keeps its data).
fn all_projects(e: &Error) -> bool {
    matches!(
        e,
        Error::RateLimit { .. }
            | Error::NotSignedIn
            | Error::NoPermission
            | Error::MissingRight { .. }
    )
}

/// What the workspace knew about a project that could not be read this time stays.
fn carry_project(known: &Workspace, project: &cockpit_core::ProjectRef, out: &mut Loaded) {
    let name = project.to_string();
    for item in known
        .items
        .iter()
        .filter(|i| i.source == Source::GitHubProject && known.sync.is_synced(&i.id))
        .filter(|i| i.project.as_ref().is_some_and(|p| p.project == name))
    {
        if !out.items.iter().any(|i| i.id == item.id) {
            out.items.push(item.clone());
        }
    }
    if let Some(meta) = known
        .sync
        .projects
        .iter()
        .find(|m| m.reference() == *project)
    {
        out.projects.push(meta.clone());
    }
}

/// Loads the items of the chosen projects as tickets. Incremental: the list pass is light (fields,
/// assignees, labels), the heavy part (comments, linked pull requests, parent, sub-issues) is
/// fetched only for issues that are new or whose `updatedAt` moved; every other issue keeps what
/// the workspace already has. The GraphQL rate limit ends the sync early (the source keeps its data
/// and pauses until the limit resets).
pub async fn projects_load(
    svc: &ProjectsService,
    known: &Workspace,
    now_s: i64,
) -> Result<Loaded, Error> {
    let client = &svc.client;
    let me = client.me().await?;
    let team_field = svc
        .team_field
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(cockpit_core::project::DEFAULT_TEAM_FIELD);
    let ctx = MapCtx {
        me: &me,
        now_s,
        team_field,
    };
    let mut out = Loaded::default();
    let mut seen: Vec<String> = Vec::new();
    let mut extras_missing = false;
    for project in &svc.projects {
        let name = project.to_string();
        let meta = match client
            .project_meta(project, svc.status_field.as_deref())
            .await
        {
            Ok(m) => m,
            Err(e) if all_projects(&e) => return Err(e),
            Err(Error::NotFound) => {
                out.warn(msg!(IntegrationsSyncProjectNotFound, project = &name));
                carry_project(known, project, &mut out);
                continue;
            }
            Err(e) => {
                out.warn(msg!(
                    IntegrationsSyncProjectStale,
                    project = &name,
                    error = e.msg()
                ));
                carry_project(known, project, &mut out);
                continue;
            }
        };
        if meta.status_def().is_none() {
            out.warn(msg!(
                IntegrationsSyncProjectNoStatus,
                project = &name,
                field = svc
                    .status_field
                    .as_deref()
                    .unwrap_or(cockpit_core::project::DEFAULT_STATUS_FIELD)
            ));
        }
        let page = match client.project_items(&meta.id).await {
            Ok(p) => p,
            Err(e) if all_projects(&e) => return Err(e),
            Err(e) => {
                out.warn(msg!(
                    IntegrationsSyncProjectStale,
                    project = &name,
                    error = e.msg()
                ));
                carry_project(known, project, &mut out);
                continue;
            }
        };
        if page.truncated {
            out.warn(msg!(
                IntegrationsSyncProjectTruncated,
                project = &name,
                n = projects::MAX_ITEMS
            ));
        }

        // The light pass: map every item; decide which ones need the heavy part.
        let mut fresh: Vec<(String, cockpit_core::Item)> = Vec::new(); // (issue node ID, item)
        let mut ready: Vec<cockpit_core::Item> = Vec::new();
        for node in &page.items {
            let Some(mut item) = projects::map_item(node, &meta, &ctx) else {
                continue;
            };
            if seen.contains(&item.id) {
                continue;
            }
            seen.push(item.id.clone());
            let Some(content_id) = item
                .project
                .as_ref()
                .filter(|p| !p.draft)
                .map(|p| p.content_id.clone())
            else {
                // A draft has no comments or timeline.
                ready.push(item);
                continue;
            };
            let old = known
                .item(&item.id)
                .filter(|o| known.sync.is_synced(&o.id))
                .filter(|o| o.updated_ms.is_some() && o.updated_ms == item.updated_ms)
                .and_then(Heavy::of_item);
            match old {
                Some(heavy) => {
                    projects::apply_heavy(&mut item, heavy);
                    ready.push(item);
                }
                None => fresh.push((content_id, item)),
            }
        }
        // The heavy pass, only for what is new or changed.
        if !fresh.is_empty() {
            let ids: Vec<String> = fresh.iter().map(|(id, _)| id.clone()).collect();
            match client.item_details(&ids).await {
                Ok((details, extras)) => {
                    extras_missing |= !extras;
                    for (content_id, mut item) in fresh {
                        if let Some(d) = details.iter().find(|d| d.id == content_id) {
                            projects::apply_details(&mut item, d, &ctx);
                        }
                        ready.push(item);
                    }
                }
                Err(e) if all_projects(&e) => return Err(e),
                Err(e) => {
                    out.warn(msg!(
                        IntegrationsSyncProjectDetailsFailed,
                        project = &name,
                        error = e.msg()
                    ));
                    for (_, mut item) in fresh {
                        // Keep the old heavy part if there is one, and ask again next time.
                        if let Some(h) = known.item(&item.id).and_then(Heavy::of_item) {
                            projects::apply_heavy(&mut item, h);
                        }
                        item.updated_ms = None;
                        ready.push(item);
                    }
                }
            }
        }
        // The order of the project, not the order the heavy pass finished in.
        let order: Vec<&str> = page.items.iter().map(|n| n.id.as_str()).collect();
        ready.sort_by_key(|i| {
            order
                .iter()
                .position(|o| Some(*o) == i.project.as_ref().map(|p| p.item_id.as_str()))
                .unwrap_or(usize::MAX)
        });
        out.items.extend(ready);
        out.projects.push(meta);
    }
    if extras_missing {
        out.warn(msg!(IntegrationsSyncProjectNoExtras));
    }
    Ok(out)
}
