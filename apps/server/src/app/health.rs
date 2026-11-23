//! Health report and the state of connections.

use super::*;

/// Whether a connection is set up: it is in the database **and** its key
/// is in the key store. The key itself is never read or printed.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Connections {
    pub github: bool,
    pub jira: bool,
}

/// Kind and state of the sign-in per service – for "Mit … anmelden" and "Trennen".
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SignIn {
    pub github: ServiceStatus,
    pub jira: ServiceStatus,
}

/// State of the sync per source: `off` (not set up or without key), `waiting`
/// (no fetch yet), `ok`, `unauthorized`, `no_permission`, `quota`,
/// `offline` or `error`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SyncState {
    pub github: String,
    pub jira: String,
    /// GitHub Projects as the source of tickets.
    pub github_projects: String,
    pub interval_s: u64,
    /// Per source that was fetched at least once: what went wrong and since when. Contains
    /// no key and no service message, only the kind of problem and times.
    pub details: Vec<SourceDetail>,
}

/// Timing of one source: `since_ms` (problem lasts since), `retry_at_ms` (rate limit: when the
/// sync asks again), `last_ok_ms` (last good fetch). All milliseconds since 1970.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SourceDetail {
    pub source: &'static str,
    pub status: String,
    pub since_ms: Option<u64>,
    pub retry_at_ms: Option<u64>,
    pub last_ok_ms: Option<u64>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
    pub connections: Connections,
    pub sign_in: SignIn,
    pub sync: SyncState,
    /// Provider, address, model and whether a key is present – never the key.
    pub endpoint: EndpointStatus,
    pub container: ContainerStatus,
    /// Whether the real agent can start and what is missing.
    pub agent: AgentStatus,
    /// Where the `.env` file with the keys lives and what to warn about – never a value.
    pub env_file: Option<secrets::EnvFileStatus>,
    /// Where the folder `config/` with configuration and database lives, what to warn about,
    /// and a hint while old files still exist at the old place.
    pub config_files: Option<crate::config::files_check::ConfigFilesStatus>,
    /// Set while saved workspaces that could not be read are kept aside in the database.
    pub unreadable_state: Option<UnreadableState>,
}

/// Saved workspaces that could not be read and are kept, unchanged, in the table
/// `workspace_unreadable`. Contains no stored data, only counts, a time and the place.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct UnreadableState {
    /// How many are kept.
    pub count: u32,
    /// When the newest was kept (`YYYY-MM-DD HH:MM:SS`, UTC).
    pub kept_at: String,
    /// German text of the notice (for readers that only know plain text).
    pub notice: String,
    /// The same notice as code and parameters; the interface shows it in its language.
    pub notice_msg: cockpit_i18n::Msg,
}

impl AppState {
    /// The notice about kept workspaces, if there are any. Blocking (database).
    fn unreadable_state(&self) -> Option<UnreadableState> {
        let db = self.inner.db.lock().unwrap_or_else(|e| e.into_inner());
        let kept = crate::db::workspaces_kept(&db)
            .inspect_err(|e| tracing::warn!("Could not check for unreadable workspaces: {e}"))
            .ok()??;
        let path = db.path().unwrap_or_default().to_string();
        let notice_msg = cockpit_i18n::msg!(
            ServerStateUnreadable,
            n = kept.count,
            at = &kept.last_kept_at,
            path = path
        );
        Some(UnreadableState {
            count: kept.count,
            kept_at: kept.last_kept_at,
            notice: notice_msg.de(),
            notice_msg,
        })
    }
}

impl AppState {
    /// Blocking (database, .env file) – do not call directly in an async handler.
    pub(super) fn health(&self) -> Health {
        // A token counts as set up when its key is in the key store - the same rule the sync
        // uses to build its clients. Jira also needs its address from the configuration.
        let has_token = |name: &str| {
            self.inner.secrets.has(name).unwrap_or_else(|e| {
                tracing::warn!("Could not check key \"{name}\": {e}");
                false
            })
        };
        let jira_configured = self.sources().jira.is_some();
        let github = self.service_status(Service::GitHub, has_token(secrets::GITHUB));
        let jira = self.service_status(
            Service::Atlassian,
            jira_configured && has_token(secrets::JIRA),
        );
        let connections = Connections {
            github: github.connected,
            jira: jira.connected,
        };
        let has_key = |name: &str| self.inner.secrets.has(name).unwrap_or(false);
        let ws = self.inner.live.copy();
        let state = |active: bool, source: cockpit_core::Source| {
            if !active {
                return "off".to_string();
            }
            ws.sync
                .state_of(source)
                .map_or("waiting", cockpit_core::SourceState::id)
                .to_string()
        };
        let q = self.sources();
        let choice = self.agent_choice();
        let endpoint = EndpointStatus {
            configured: choice.endpoint.is_some(),
            provider: choice.provider,
            url: choice.endpoint.as_ref().and_then(|e| e.url.clone()),
            model: choice.endpoint.as_ref().and_then(|e| e.model.clone()),
            key: has_key(secrets::AGENT_KEY) || has_key(secrets::LITELLM),
        };
        let container = self.container_status();
        let agent = self.agent_status();
        Health {
            status: "ok",
            version: VERSION,
            sync: SyncState {
                github: state(github.connected, cockpit_core::Source::GitHub),
                jira: state(
                    q.jira.is_some() && jira.connected,
                    cockpit_core::Source::Jira,
                ),
                github_projects: state(
                    q.github_projects
                        .as_ref()
                        .is_some_and(|p| p.tickets_from.uses_projects() && !p.projects.is_empty())
                        && github.connected,
                    cockpit_core::Source::GitHubProject,
                ),
                interval_s: q.interval.as_secs(),
                details: ws
                    .sync
                    .sources
                    .iter()
                    .map(|st| SourceDetail {
                        source: match st.source {
                            cockpit_core::Source::Jira => "jira",
                            cockpit_core::Source::GitHub => "github",
                            cockpit_core::Source::GitHubProject => "github_projects",
                        },
                        status: st.id().to_string(),
                        since_ms: st.since_ms,
                        retry_at_ms: match st.problem {
                            Some(cockpit_core::Problem::RateLimited { until_ms }) => until_ms,
                            _ => None,
                        },
                        last_ok_ms: st.last_ok_ms,
                    })
                    .collect(),
            },
            connections,
            sign_in: SignIn { github, jira },
            endpoint,
            container,
            agent,
            env_file: self.inner.secrets.env_file().map(secrets::env_file_status),
            config_files: self.inner.settings.config_dir.as_deref().map(|dir| {
                crate::config::files_check::status(
                    dir,
                    &self.inner.settings.legacy_files,
                    self.inner.settings.legacy_env.as_deref(),
                )
            }),
            unreadable_state: self.unreadable_state(),
        }
    }
}

pub(super) async fn health(State(state): State<AppState>) -> Result<Json<Health>, StatusCode> {
    tokio::task::spawn_blocking(move || state.health())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// The key of the endpoint from the .env file: `agent-key`, alternatively the older `litellm`.
/// Blockierend.
pub(crate) fn agent_key_read(
    secrets: &dyn crate::secrets::SecretStore,
) -> Result<Option<cockpit_integrations::Secret>, crate::secrets::SecretError> {
    match secrets.get(secrets::AGENT_KEY)? {
        Some(k) => Ok(Some(k)),
        None => secrets.get(secrets::LITELLM),
    }
}

impl AppState {
    /// Where the calls to the endpoint go: the entered address, otherwise directly Anthropic.
    pub(crate) fn endpoint_target(&self) -> cockpit_integrations::endpoint::Target {
        let choice = self.agent_choice();
        cockpit_integrations::endpoint::Target::for_provider(
            choice.provider.openai_compatible(),
            choice.endpoint.as_ref().and_then(|e| e.url.as_deref()),
            self.inner.settings.anthropic_api.as_deref(),
        )
    }

    /// The key of the endpoint. `Err` is already a finished response.
    pub(super) async fn endpoint_key(
        &self,
    ) -> Result<Option<cockpit_integrations::Secret>, Box<Response>> {
        let secrets = self.inner.secrets.clone();
        match tokio::task::spawn_blocking(move || agent_key_read(secrets.as_ref())).await {
            Ok(Ok(k)) => Ok(k),
            Ok(Err(e)) => {
                tracing::warn!("Agent endpoint key not readable: {e}");
                Err(Box::new(error_msg(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    cockpit_i18n::msg!(ServerMiscEnvNotReadable),
                )))
            }
            Err(_) => Err(Box::new(StatusCode::INTERNAL_SERVER_ERROR.into_response())),
        }
    }
}
