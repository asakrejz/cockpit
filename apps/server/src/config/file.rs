//! The shape of config.toml as it is read, and the checks of its parts.

use super::*;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct File {
    pub(super) port: Option<u16>,
    pub(super) dist: Option<PathBuf>,
    pub(super) db: Option<PathBuf>,
    #[serde(alias = "intervall_s")]
    pub(super) interval_s: Option<u64>,
    pub(super) github: Option<FileGitHub>,
    pub(super) jira: Option<FileJira>,
    pub(super) github_oauth: Option<FileOAuth>,
    pub(super) atlassian: Option<FileAtlassian>,
    pub(super) seed: Option<String>,
    pub(super) litellm: Option<FileEndpoint>,
    pub(super) agent: Option<FileAgent>,
    pub(super) server: Option<FileServer>,
    pub(super) sync: Option<FileSync>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileServer {
    #[serde(default)]
    pub(super) webhooks: bool,
    pub(super) public_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileSync {
    pub(super) webhook_fallback_interval_s: Option<u64>,
    /// At most this many open pull requests per repo (default 200).
    pub(super) max_prs_per_repo: Option<usize>,
    /// At most this many open pull requests over all repos (default 500).
    pub(super) max_prs_total: Option<usize>,
}

/// `[litellm]` (older) and `[agent.endpoint]` have the same fields.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileEndpoint {
    pub(super) url: Option<String>,
    #[serde(alias = "modell")]
    pub(super) model: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileAgentRepo {
    pub(super) repo: String,
    pub(super) base_branch: Option<String>,
    #[serde(default)]
    pub(super) projects: Vec<String>,
    /// `claude/{ticket}-{slug}` if missing.
    #[serde(default)]
    pub(super) branch_pattern: Option<String>,
    /// Language of the pull request title and text: `en` (default) or `de`.
    #[serde(default)]
    pub(super) pr_language: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileAgent {
    pub(super) container: Option<String>,
    pub(super) repo: Option<String>,
    #[serde(default)]
    pub(super) repos: Vec<FileAgentRepo>,
    pub(super) dir: Option<PathBuf>,
    #[serde(alias = "anbieter")]
    pub(super) provider: Option<String>,
    #[serde(alias = "endpunkt")]
    pub(super) endpoint: Option<FileEndpoint>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileOAuth {
    pub(super) client_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileAtlassian {
    pub(super) client_id: String,
    pub(super) site: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileGitHub {
    pub(super) api_url: Option<String>,
    #[serde(default)]
    pub(super) repos: Vec<String>,
    #[serde(default)]
    pub(super) auth: Option<cockpit_core::AuthChoice>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileJira {
    #[serde(default)]
    pub(super) site: String,
    #[serde(default)]
    pub(super) email: String,
    #[serde(default)]
    pub(super) projects: Vec<String>,
    pub(super) jql: Option<String>,
    #[serde(default)]
    pub(super) hidden_fields: Vec<String>,
    #[serde(default)]
    pub(super) field_order: Vec<String>,
    #[serde(default)]
    pub(super) auth: Option<cockpit_core::AuthChoice>,
}

/// `docker` or `podman`, otherwise an error.
pub(super) fn runtime(text: &str) -> Result<String, ConfigError> {
    match text.trim().to_lowercase().as_str() {
        l @ ("docker" | "podman") => Ok(l.to_string()),
        other => Err(ConfigError::Wert(msg!(
            ServerConfigRuntimeUnsupported,
            value = other
        ))),
    }
}

/// `owner/name` made of letters, digits, `.`, `-` and `_`.
pub(super) fn repo_valid(r: &str) -> bool {
    let teil = |t: &str| {
        !t.is_empty()
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    r.split_once('/')
        .is_some_and(|(o, n)| teil(o) && teil(n) && !n.contains('/'))
}

/// `[[agent.repos]]`: every repo valid and once, every project a valid key and routed to one
/// repo only (otherwise the rule would be ambiguous).
pub(super) fn agent_repos_read(list: &[FileAgentRepo]) -> Result<Vec<AgentRepo>, ConfigError> {
    let mut out: Vec<AgentRepo> = Vec::new();
    for r in list {
        let repo = r.repo.trim().to_string();
        if !crate::agent_git::repo_name_ok(&repo) {
            return Err(ConfigError::Wert(msg!(
                ServerConfigReposRepoForm,
                repo = &repo
            )));
        }
        if out.iter().any(|o| o.repo == repo) {
            return Err(ConfigError::Wert(msg!(
                ServerConfigReposDuplicate,
                repo = &repo
            )));
        }
        let base = r
            .base_branch
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .unwrap_or(cockpit_core::repos::DEFAULT_BASE);
        if !crate::agent_git::base_branch_ok(base) {
            return Err(ConfigError::Wert(msg!(
                ServerConfigReposBaseBranch,
                base = base,
                repo = &repo
            )));
        }
        let mut projects: Vec<String> = Vec::new();
        for p in &r.projects {
            let p = p.trim().to_ascii_uppercase();
            if !is_project_key(&p) {
                return Err(ConfigError::Wert(msg!(
                    ServerConfigReposProjectKey,
                    repo = &repo,
                    project = &p
                )));
            }
            if let Some(other) = out.iter().find(|o| o.projects.contains(&p)) {
                return Err(ConfigError::Wert(msg!(
                    ServerConfigReposProjectTaken,
                    project = &p,
                    owner = &other.repo,
                    repo = &repo
                )));
            }
            if !projects.contains(&p) {
                projects.push(p);
            }
        }
        let pattern = r
            .branch_pattern
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .unwrap_or(cockpit_core::branch::DEFAULT_PATTERN);
        cockpit_core::branch::validate(pattern).map_err(|m| {
            ConfigError::Wert(msg!(
                ServerConfigReposPattern,
                repo = &repo,
                reason = Msg::raw(m)
            ))
        })?;
        let pr_language = match r.pr_language.as_deref().map(str::trim) {
            None | Some("") => cockpit_i18n::Lang::En,
            Some(code) => cockpit_i18n::Lang::from_code(code).ok_or_else(|| {
                ConfigError::Wert(msg!(
                    ServerConfigReposPrLanguage,
                    repo = &repo,
                    value = code
                ))
            })?,
        };
        out.push(AgentRepo {
            repo,
            base_branch: base.to_string(),
            projects,
            branch_pattern: pattern.to_string(),
            pr_language,
        });
    }
    Ok(out)
}

/// `Ok(None)` if the file does not exist.
pub(super) fn read_optional(p: &Path) -> Result<Option<String>, (PathBuf, std::io::Error)> {
    match std::fs::read_to_string(p) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err((p.to_path_buf(), e)),
    }
}
