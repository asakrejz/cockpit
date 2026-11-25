use super::*;

impl AppState {
    /// Builds the services that are set up **and** have a key. Blocking
    /// (.env file).
    pub(crate) fn services_build(&self) -> Services {
        let inner = &self.inner;
        let sources = self.sources();
        let mut services = Services::default();
        {
            let g = sources.github_config();
            // The sign-in via OAuth goes before the token.
            if let Some((t, app)) = self.github_credentials() {
                match GitHubClient::new(&g.api_url, t) {
                    Ok(client) => {
                        services.github = Some(GitHubService {
                            client: client.with_limits(sources.pr_limits).with_app(app),
                            repos: g.repos.clone(),
                        })
                    }
                    Err(e) => tracing::warn!("GitHub: {e}"),
                }
            }
            if let (Some(p), Some((t, app))) = (&sources.github_projects, self.github_credentials())
                && p.tickets_from.uses_projects()
                && !p.projects.is_empty()
            {
                match GitHubClient::new(&g.api_url, t) {
                    Ok(client) => {
                        services.projects = Some(cockpit_integrations::ProjectsService {
                            client: client.with_app(app),
                            projects: p.projects.clone(),
                            status_field: p.status_field.clone(),
                            team_field: p.team_field.clone(),
                        })
                    }
                    Err(e) => tracing::warn!("GitHub Projects: {e}"),
                }
            }
        }
        if let Some(j) = sources
            .jira
            .as_ref()
            .filter(|_| sources.tickets_from().uses_jira())
            && let Some(client) = self.jira_client()
        {
            // A project chosen in the app takes precedence over `projects` and `jql` of the file.
            let live = inner.live.copy();
            let query = jql_for_selection(
                &j.projects,
                j.jql.as_deref(),
                live.jira_project.as_deref(),
                live.inbox_filter.sprint,
            );
            match (client, query) {
                (Ok(client), Ok(query)) => {
                    services.jira = Some(JiraService {
                        client: client.with_team_field(j.team_field.as_deref()),
                        jql: query.jql,
                        in_project: query.in_selection,
                        fields: cockpit_integrations::jira::FieldPrefs {
                            hidden: j.hidden_fields.clone(),
                            order: j.field_order.clone(),
                        },
                    })
                }
                (Err(e), _) | (_, Err(e)) => tracing::warn!("Jira: {e}"),
            }
        }
        services
    }

    /// The Jira client: via OAuth (through the Atlassian gateway), otherwise with email and API token.
    /// `None` if Jira is not set up or the access is missing. Blocking.
    pub(crate) fn jira_client(&self) -> Option<Result<JiraClient, cockpit_integrations::Error>> {
        let sources = self.sources();
        let token = sources
            .jira
            .as_ref()
            .filter(|j| !j.site.is_empty())
            .and_then(|j| self.token(secrets::JIRA).map(|t| (j, t)));
        let kind = AuthChoice::pick(
            sources.jira_auth,
            token.is_some(),
            self.oauth_state(Service::Atlassian),
        );
        match kind {
            AuthKind::OAuth => self.jira_oauth_client(),
            AuthKind::Token => token.map(|(j, t)| JiraClient::new(&j.site, &j.email, t)),
            AuthKind::None => None,
        }
    }

    /// The access to GitHub and whether it is the sign-in via the app. The picked method decides
    /// ([`cockpit_core::AuthChoice::pick`], the same rule as
    /// [`cockpit_core::ServiceStatus::resolve`]); without a choice a working sign-in goes before
    /// the token. Blocking.
    pub(crate) fn github_credentials(&self) -> Option<(cockpit_integrations::Secret, bool)> {
        let token = self.token(secrets::GITHUB);
        let kind = AuthChoice::pick(
            self.sources().github_auth,
            token.is_some(),
            self.oauth_state(Service::GitHub),
        );
        match kind {
            AuthKind::OAuth => self.oauth_tokens(Service::GitHub).map(|t| (t.access, true)),
            AuthKind::Token => token.map(|t| (t, false)),
            AuthKind::None => None,
        }
    }

    /// The access to GitHub (see [`Self::github_credentials`]). Blocking.
    pub(crate) fn github_access(&self) -> Option<cockpit_integrations::Secret> {
        self.github_credentials().map(|(t, _)| t)
    }

    /// A key from the .env file, empty counts as missing. Blocking.
    pub(crate) fn token(&self, name: &str) -> Option<cockpit_integrations::Secret> {
        match self.inner.secrets.get(name) {
            Ok(t) => t.filter(|t| !t.is_empty()),
            Err(e) => {
                tracing::warn!("Key \"{name}\" not readable: {e}");
                None
            }
        }
    }
}
