//! Fetching for the sync: gets tickets, pull requests, comments, checks and diffs via
//! the clients and packs them as [`Remote`] for `cockpit_core::sync::merge`. This module holds
//! the configured services and the way out for outward calls.
//!
//! If a source fails, its state in the workspace stays unchanged; the error becomes the
//! [`Problem`] of the source.

use cockpit_core::{Loaded, Outcome, Remote, Source, SourceData, Workspace};
use cockpit_i18n::msg;

use crate::Error;
use crate::github::GitHubClient;
use crate::jira::{self, JiraClient};
use crate::outbound::{Outbound, Route, Targets};

mod load_github;
mod load_jira;
mod load_projects;

pub use load_github::github_load;
pub use load_jira::jira_load;
pub use load_projects::projects_load;

pub struct GitHubService {
    pub client: GitHubClient,
    /// `owner/name`
    pub repos: Vec<String>,
}

pub struct JiraService {
    pub client: JiraClient,
    pub jql: String,
    /// The query fetches all tickets of a project ([`jira::jql_for_project`]): whatever is neither
    /// assigned nor watched then shows up with the reason `reason.in_project`.
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

    /// What the plan of an action may use. `default_repo` is the default repo of the agent
    /// (see [`Targets::default_repo`]).
    pub fn targets(&self, default_repo: Option<String>) -> Targets {
        Targets {
            github: self.github.is_some(),
            jira: self.jira.is_some(),
            projects: self.projects.is_some(),
            default_repo,
        }
    }

    /// Runs the call at the responsible service. This is the way out for every outward call: the
    /// route of the call picks the service, and its client sends it.
    pub async fn execute(&self, a: &Outbound) -> Result<(), Error> {
        match a.route() {
            Route::Jira => match &self.jira {
                Some(j) => j.client.execute(a).await,
                None => Err(Error::Coded(msg!(IntegrationsJiraNotSetUp))),
            },
            // Changes at GitHub Projects go through the client of the project source.
            Route::Projects => match (&self.projects, &self.github) {
                (Some(p), _) => p.client.execute(a).await,
                (None, Some(g)) => g.client.execute(a).await,
                (None, None) => Err(Error::Coded(msg!(IntegrationsPlanProjectsOff))),
            },
            Route::GitHub => match &self.github {
                Some(g) => g.client.execute(a).await,
                None => Err(Error::Coded(msg!(IntegrationsGithubNotSetUp))),
            },
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
            let res = github_load(&g.client, &g.repos, known).await;
            sources.push(as_source(Source::GitHub, res, now_ms));
        }
        if let Some(j) = self
            .jira
            .as_ref()
            .filter(|_| !paused.contains(&Source::Jira))
        {
            let res = jira_load(&j.client, &j.jql, j.in_project, &j.fields).await;
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
