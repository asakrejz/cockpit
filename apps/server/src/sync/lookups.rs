use super::*;

impl AppState {
    /// The Jira projects that the key may see – for the selection in the inbox.
    /// Without Jira or key: `active: false`.
    pub async fn jira_projects(&self) -> Result<JiraProjectsResponse, ActionFailure> {
        if self.sources().jira.is_none() {
            return Ok(JiraProjectsResponse::default());
        }
        self.oauth_refresh().await;
        let s = self.clone();
        let client = tokio::task::spawn_blocking(move || s.jira_client())
            .await
            .map_err(|_| ActionFailure::Internal)?;
        let Some(client) = client else {
            return Ok(JiraProjectsResponse::default());
        };
        let client =
            client.map_err(|e| ActionFailure::Plan(msg!(ServerRunJiraError, error = e.msg())))?;
        let projects = client.projects().await.map_err(|e| {
            tracing::warn!("Jira projects: {e}");
            ActionFailure::Service(msg!(ServerRunJiraNoProjects, error = e.msg()))
        })?;
        Ok(JiraProjectsResponse {
            active: true,
            projects: projects
                .into_iter()
                .map(|p| JiraProject {
                    key: p.key,
                    name: p.name,
                })
                .collect(),
        })
    }

    /// People Jira lets you assign tickets of `project` to, whose name matches `query`.
    /// Read only. Without Jira or key: `active: false`.
    pub async fn jira_assignable(
        &self,
        project: &str,
        query: &str,
    ) -> Result<AssignableResponse, ActionFailure> {
        if self.sources().jira.is_none() {
            return Ok(AssignableResponse::default());
        }
        self.oauth_refresh().await;
        let s = self.clone();
        let client = tokio::task::spawn_blocking(move || s.jira_client())
            .await
            .map_err(|_| ActionFailure::Internal)?;
        let Some(client) = client else {
            return Ok(AssignableResponse::default());
        };
        let client =
            client.map_err(|e| ActionFailure::Plan(msg!(ServerRunJiraError, error = e.msg())))?;
        let people = client
            .assignable_people(project, query)
            .await
            .map_err(|e| {
                tracing::warn!("Jira assignable users: {e}");
                ActionFailure::Service(msg!(ServerRunJiraNoPeople, error = e.msg()))
            })?;
        Ok(AssignableResponse {
            active: true,
            people,
        })
    }

    /// People GitHub lets you assign issues of `repo` to, whose login matches `query`. Read only.
    /// Without GitHub or key: `active: false`.
    pub async fn github_assignable(
        &self,
        repo: &str,
        query: &str,
    ) -> Result<AssignableResponse, ActionFailure> {
        self.oauth_refresh().await;
        let s = self.clone();
        let credentials = tokio::task::spawn_blocking(move || s.github_credentials())
            .await
            .map_err(|_| ActionFailure::Internal)?;
        let Some((token, app)) = credentials else {
            return Ok(AssignableResponse::default());
        };
        let client = GitHubClient::new(&self.sources().github_config().api_url, token)
            .map_err(|e| ActionFailure::Plan(msg!(ServerRunGithubError, error = e.msg())))?
            .with_app(app);
        let me = client.me().await.unwrap_or_default();
        let logins = client.assignees(repo, query).await.map_err(|e| {
            tracing::warn!("GitHub assignees: {e}");
            ActionFailure::Service(msg!(ServerRunGithubNoPeople, error = e.msg()))
        })?;
        Ok(AssignableResponse {
            active: true,
            people: logins
                .into_iter()
                .map(|l| {
                    if !me.is_empty() && l.eq_ignore_ascii_case(&me) {
                        cockpit_core::Person::me()
                    } else {
                        cockpit_core::Person::new(&l, &cockpit_integrations::text::initials(&l))
                    }
                })
                .collect(),
        })
    }
}
