use super::*;

/// Jira and GitHub as configured.
#[derive(Clone, Debug)]
pub struct Sources {
    pub github: Option<GitHubConfig>,
    pub jira: Option<JiraConfig>,
    /// GitHub Projects as the source of tickets.
    pub github_projects: Option<GitHubProjectsConfig>,
    /// The picked sign-in method per service (`None`: OAuth first, then token).
    pub github_auth: Option<cockpit_core::AuthChoice>,
    pub jira_auth: Option<cockpit_core::AuthChoice>,
    /// Sign-in via OAuth (client IDs only; the secrets are in the .env file).
    pub github_oauth: Option<OAuthConfig>,
    pub atlassian: Option<AtlassianConfig>,
    /// Addresses of the OAuth services (tests insert imitations).
    pub endpoints: Endpoints,
    pub interval: Duration,
    /// How many open pull requests the GitHub sync loads.
    pub pr_limits: PrLimits,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            github: None,
            jira: None,
            github_projects: None,
            github_auth: None,
            jira_auth: None,
            github_oauth: None,
            atlassian: None,
            endpoints: Endpoints::default(),
            interval: Duration::from_secs(DEFAULT_INTERVAL_S),
            pr_limits: PrLimits::default(),
        }
    }
}

impl Sources {
    pub fn from_config(config: &Config) -> Self {
        Self {
            github: config.github.clone(),
            jira: config.jira.clone(),
            github_projects: config.github_projects.clone(),
            github_auth: config.github_auth,
            jira_auth: config.jira_auth,
            github_oauth: config.github_oauth.clone(),
            atlassian: config.atlassian.clone(),
            endpoints: Endpoints::default(),
            interval: Duration::from_secs(config.interval_s),
            pr_limits: config.github_pr_limits,
        }
    }

    /// The GitHub settings; without a `[github]` section the defaults (address of github.com, no
    /// watched repos) apply, because the sync needs no repo list.
    pub fn github_config(&self) -> GitHubConfig {
        self.github.clone().unwrap_or_default()
    }

    /// Where tickets come from (Jira alone unless `[github_projects]` says otherwise).
    pub fn tickets_from(&self) -> cockpit_core::TicketsFrom {
        self.github_projects
            .as_ref()
            .map_or(cockpit_core::TicketsFrom::Jira, |p| p.tickets_from)
    }
}
