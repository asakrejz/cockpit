//! The shape of config.toml as it is read, and the checks of its parts.

use super::*;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct File {
    pub(super) port: Option<u16>,
    pub(super) dist: Option<PathBuf>,
    pub(super) db: Option<PathBuf>,
    pub(super) interval_s: Option<u64>,
    pub(super) github: Option<FileGitHub>,
    pub(super) jira: Option<FileJira>,
    pub(super) github_projects: Option<FileGitHubProjects>,
    pub(super) github_oauth: Option<FileOAuth>,
    pub(super) atlassian: Option<FileAtlassian>,
    pub(super) seed: Option<String>,
    pub(super) agent: Option<FileAgent>,
    pub(super) server: Option<FileServer>,
    pub(super) sync: Option<FileSync>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileGitHubProjects {
    /// `jira`, `github_projects` or `both`; missing = `both` (the section exists).
    #[serde(default)]
    pub(super) tickets_from: Option<String>,
    /// `owner/number`
    #[serde(default)]
    pub(super) projects: Vec<String>,
    #[serde(default)]
    pub(super) status_field: Option<String>,
    #[serde(default)]
    pub(super) team_field: Option<String>,
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

/// `[agent.endpoint]`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileEndpoint {
    pub(super) url: Option<String>,
    pub(super) model: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileAgentRepo {
    pub(super) repo: String,
    pub(super) base_branch: Option<String>,
    /// Accepted so that a file with `projects = [...]` still starts; the value is ignored and the
    /// setup assistant leaves the key out when it saves the file.
    #[serde(default, rename = "projects")]
    pub(super) _projects: serde::de::IgnoredAny,
    /// `claude/{ticket}-{slug}` if missing.
    #[serde(default)]
    pub(super) branch_pattern: Option<String>,
    /// Language of the pull request title and text: `en` (default) or `de`.
    #[serde(default)]
    pub(super) pr_language: Option<String>,
    /// `false`: the entry only keeps the settings of the repo; the sync does not load all of its
    /// open pull requests. Written by the app on the first use of a repo.
    #[serde(default)]
    pub(super) watch: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileAgent {
    pub(super) container: Option<String>,
    pub(super) repo: Option<String>,
    #[serde(default)]
    pub(super) repos: Vec<FileAgentRepo>,
    pub(super) dir: Option<PathBuf>,
    pub(super) provider: Option<String>,
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
    pub(super) team_field: Option<String>,
    #[serde(default)]
    pub(super) auth: Option<cockpit_core::AuthChoice>,
}

/// Reads the text of the file. A key that does not exist (a typo, or a name this version does not
/// have) is an error that names the key and its section; it is never skipped silently.
pub(super) fn file_read(text: &str) -> Result<File, ConfigError> {
    toml::from_str(text).map_err(|e| {
        unknown_key(text, &e).unwrap_or_else(|| ConfigError::Toml(e.message().to_string()))
    })
}

/// `Some` if the parser refused the text because of a key the file types do not have.
fn unknown_key(text: &str, e: &toml::de::Error) -> Option<ConfigError> {
    let key = e
        .message()
        .strip_prefix("unknown field `")?
        .split('`')
        .next()?;
    let section = e
        .span()
        .and_then(|span| section_of_key(text, span.start))
        .unwrap_or_default();
    Some(ConfigError::UnknownKey {
        section,
        key: key.to_string(),
        file: PathBuf::from(location::CONFIG_FILE),
    })
}

/// The table (`agent.endpoint`) in which the key that starts at byte `at` stands; empty at the top.
fn section_of_key(text: &str, at: usize) -> Option<String> {
    let doc = toml_edit::Document::parse(text).ok()?;
    let mut path = Vec::new();
    find_section(doc.as_table(), &mut path, at).then(|| path.join("."))
}

/// Walks `table` and its sub-tables; `path` ends as the path of the table that has the key at `at`.
fn find_section(table: &dyn toml_edit::TableLike, path: &mut Vec<String>, at: usize) -> bool {
    use toml_edit::{Item, Value};
    for (name, item) in table.iter() {
        let is_here = table
            .get_key_value(name)
            .and_then(|(key, _)| key.span())
            .is_some_and(|span| span.contains(&at));
        if is_here {
            return true;
        }
        path.push(name.to_string());
        let found = match item {
            Item::Table(t) => find_section(t, path, at),
            Item::Value(Value::InlineTable(t)) => find_section(t, path, at),
            Item::ArrayOfTables(all) => all.iter().any(|t| find_section(t, path, at)),
            _ => false,
        };
        if found {
            return true;
        }
        path.pop();
    }
    false
}

/// `docker` or `podman`, otherwise an error.
pub(super) fn runtime(text: &str) -> Result<String, ConfigError> {
    match text.trim().to_lowercase().as_str() {
        l @ ("docker" | "podman") => Ok(l.to_string()),
        other => Err(ConfigError::InvalidValue(msg!(
            ServerConfigRuntimeUnsupported,
            value = other
        ))),
    }
}

/// `[[agent.repos]]`: every repo valid and once.
pub(super) fn agent_repos_read(list: &[FileAgentRepo]) -> Result<Vec<AgentRepo>, ConfigError> {
    let mut out: Vec<AgentRepo> = Vec::new();
    for r in list {
        let repo = r.repo.trim().to_string();
        if !cockpit_core::repo_name_valid(&repo) {
            return Err(ConfigError::InvalidValue(msg!(
                ServerConfigReposRepoForm,
                repo = &repo
            )));
        }
        if out.iter().any(|o| o.repo == repo) {
            return Err(ConfigError::InvalidValue(msg!(
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
            return Err(ConfigError::InvalidValue(msg!(
                ServerConfigReposBaseBranch,
                base = base,
                repo = &repo
            )));
        }
        let pattern = r
            .branch_pattern
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .unwrap_or(cockpit_core::branch::DEFAULT_PATTERN);
        cockpit_core::branch::validate(pattern).map_err(|m| {
            ConfigError::InvalidValue(msg!(ServerConfigReposPattern, repo = &repo, reason = m))
        })?;
        let pr_language = match r.pr_language.as_deref().map(str::trim) {
            None | Some("") => cockpit_i18n::Lang::En,
            Some(code) => cockpit_i18n::Lang::from_code(code).ok_or_else(|| {
                ConfigError::InvalidValue(msg!(
                    ServerConfigReposPrLanguage,
                    repo = &repo,
                    value = code
                ))
            })?,
        };
        out.push(AgentRepo {
            repo,
            base_branch: base.to_string(),
            branch_pattern: pattern.to_string(),
            pr_language,
            watch: r.watch.unwrap_or(true),
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
