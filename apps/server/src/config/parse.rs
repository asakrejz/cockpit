use super::*;

impl Config {
    /// Defaults → file content → environment. `file` is the text of the TOML file (if
    /// present), `env` delivers values of environment variables.
    pub fn parse(
        file: Option<&str>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let d: File = match file {
            Some(text) => file_read(text)?,
            None => File::default(),
        };
        let mut c = Self::defaults();
        if let Some(p) = d.port {
            c.port = p;
        }
        if let Some(p) = d.dist {
            c.dist = p;
        }
        if let Some(p) = d.db {
            c.db = p;
        }
        if let Some(i) = d.interval_s {
            if i < MIN_INTERVAL_S {
                return Err(ConfigError::InvalidValue(msg!(
                    ServerConfigIntervalShort,
                    interval = i,
                    min = MIN_INTERVAL_S
                )));
            }
            c.interval_s = i;
        }
        if let Some(g) = d.github {
            c.github_auth = g.auth;
            for r in &g.repos {
                if !cockpit_core::repo_name_valid(r) {
                    return Err(ConfigError::InvalidValue(msg!(
                        ServerConfigGithubRepoForm,
                        repo = r
                    )));
                }
            }
            c.github = Some(GitHubConfig {
                api_url: g.api_url.unwrap_or_else(|| DEFAULT_GITHUB_API.into()),
                repos: g.repos,
            });
        }
        let client_id = |section: &str, id: String| {
            let id = id.trim().to_string();
            if id.is_empty() || id.chars().any(char::is_whitespace) {
                Err(ConfigError::InvalidValue(msg!(
                    ServerConfigClientIdInvalid,
                    who = section
                )))
            } else {
                Ok(id)
            }
        };
        if let Some(o) = d.github_oauth {
            c.github_oauth = Some(OAuthConfig {
                client_id: client_id("[github_oauth]", o.client_id)?,
            });
        }
        if let Some(a) = d.atlassian {
            c.atlassian = Some(AtlassianConfig {
                client_id: client_id("[atlassian]", a.client_id)?,
                site: a
                    .site
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            });
        }
        // With Atlassian sign-in Jira is enough without own details.
        let jira = d.jira.or_else(|| {
            c.atlassian.as_ref().map(|_| FileJira {
                site: String::new(),
                email: String::new(),
                projects: Vec::new(),
                jql: None,
                hidden_fields: Vec::new(),
                field_order: Vec::new(),
                team_field: None,
                auth: None,
            })
        });
        if let Some(j) = jira {
            c.jira_auth = j.auth;
            if c.atlassian.is_none() && (j.site.trim().is_empty() || j.email.trim().is_empty()) {
                return Err(ConfigError::InvalidValue(msg!(ServerConfigJiraNeedsSite)));
            }
            for p in &j.projects {
                if cockpit_integrations::jira::jql_for(std::slice::from_ref(p), None).is_err() {
                    return Err(ConfigError::InvalidValue(msg!(
                        ServerConfigJiraProjectInvalid,
                        project = p
                    )));
                }
            }
            c.jira = Some(JiraConfig {
                site: j.site,
                email: j.email,
                projects: j.projects,
                jql: j.jql.filter(|q| !q.trim().is_empty()),
                hidden_fields: clean_list(j.hidden_fields),
                field_order: clean_list(j.field_order),
                team_field: j
                    .team_field
                    .map(|f| f.trim().to_string())
                    .filter(|f| !f.is_empty()),
            });
        }

        if let Some(p) = d.github_projects {
            let tickets_from = match p.tickets_from.as_deref().map(str::trim) {
                None | Some("") => cockpit_core::TicketsFrom::Both,
                Some(t) => cockpit_core::TicketsFrom::from_id(t).ok_or_else(|| {
                    ConfigError::InvalidValue(msg!(ServerConfigTicketsFromUnknown, value = t))
                })?,
            };
            let mut projects = Vec::new();
            for text in &p.projects {
                let r = cockpit_core::ProjectRef::parse(text).ok_or_else(|| {
                    ConfigError::InvalidValue(msg!(ServerConfigProjectForm, project = text))
                })?;
                if !projects.contains(&r) {
                    projects.push(r);
                }
            }
            let name =
                |v: Option<String>| v.map(|f| f.trim().to_string()).filter(|f| !f.is_empty());
            c.github_projects = Some(GitHubProjectsConfig {
                tickets_from,
                projects,
                status_field: name(p.status_field),
                team_field: name(p.team_field),
            });
        }

        if let Some(text) = d.seed {
            c.seed = Seed::from_text(&text)?;
        }
        let mut endpoint = None;
        if let Some(a) = d.agent {
            if let Some(text) = a.provider {
                c.provider = Provider::from_id(&text).ok_or_else(|| {
                    ConfigError::InvalidValue(msg!(
                        ServerConfigProviderUnknown,
                        provider = text.trim()
                    ))
                })?;
            }
            endpoint = a.endpoint;
            c.container = a.container.map(|t| runtime(&t)).transpose()?;
            c.agent_repo = a
                .repo
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty());
            if let Some(r) = &c.agent_repo
                && !cockpit_core::repo_name_valid(r)
            {
                return Err(ConfigError::InvalidValue(msg!(
                    ServerConfigAgentRepoForm,
                    repo = r
                )));
            }
            c.agent_repos = agent_repos_read(&a.repos)?;
            c.agent_dir = a.dir;
        }
        // Watched repos (all open pull requests are loaded): `[github] repos` plus the `[[agent.repos]]`
        // entries without `watch = false`. Other repos are found by the search only.
        if c.agent_repos.iter().any(|r| r.watch) {
            let g = c.github.get_or_insert_with(GitHubConfig::default);
            for r in c.agent_repos.iter().filter(|r| r.watch) {
                if !g.repos.contains(&r.repo) {
                    g.repos.push(r.repo.clone());
                }
            }
        }
        if let Some(e) = endpoint {
            let url = e.url.map(|u| u.trim().trim_end_matches('/').to_string());
            let url = match url.filter(|u| !u.is_empty()) {
                None => None,
                Some(u) => {
                    let valid = u
                        .strip_prefix("http://")
                        .or_else(|| u.strip_prefix("https://"))
                        .is_some_and(|rest| {
                            !rest.is_empty() && !rest.contains(char::is_whitespace)
                        });
                    if !valid {
                        return Err(ConfigError::InvalidValue(msg!(
                            ServerConfigEndpointUrlScheme
                        )));
                    }
                    Some(u)
                }
            };
            let model = e
                .model
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty());
            if url.is_some() || model.is_some() {
                c.endpoint = Some(EndpointConfig { url, model });
            }
        }
        if let Some(w) = d.server {
            c.webhooks = w.webhooks;
            c.public_url = Some(
                cockpit_core::setup::public_url_read(w.public_url.as_deref().unwrap_or(""))
                    .map_err(ConfigError::InvalidValue)?,
            )
            .filter(|u| !u.is_empty());
        }
        if let Some(s) = &d.sync {
            for (name, value) in [
                ("max_prs_per_repo", s.max_prs_per_repo),
                ("max_prs_total", s.max_prs_total),
            ] {
                if let Some(n) = value
                    && !(1..=MAX_PRS_LIMIT).contains(&n)
                {
                    return Err(ConfigError::InvalidValue(msg!(
                        ServerConfigSyncLimit,
                        name = name,
                        value = n,
                        max = MAX_PRS_LIMIT
                    )));
                }
            }
            if let Some(n) = s.max_prs_per_repo {
                c.github_pr_limits.per_repo = n;
            }
            if let Some(n) = s.max_prs_total {
                c.github_pr_limits.total = n;
            }
        }
        if let Some(i) = d.sync.and_then(|s| s.webhook_fallback_interval_s) {
            if !(c.interval_s..=WEBHOOK_FALLBACK_LIMIT_S).contains(&i) {
                return Err(ConfigError::InvalidValue(msg!(
                    ServerConfigWebhookFallbackRange,
                    value = i,
                    interval = c.interval_s,
                    max = WEBHOOK_FALLBACK_LIMIT_S
                )));
            }
            c.webhook_fallback_interval_s = Some(i);
        }
        if let Some(p) = env("COCKPIT_AGENT_DIR").filter(|s| !s.trim().is_empty()) {
            c.agent_dir = Some(PathBuf::from(p.trim()));
        }
        if let Some(p) = env("COCKPIT_GIT_BASE").filter(|s| !s.trim().is_empty()) {
            c.git_base = Some(p.trim().trim_end_matches('/').to_string());
        }

        if let Some(p) = env("COCKPIT_SEED").filter(|s| !s.trim().is_empty()) {
            c.seed = Seed::from_text(&p)?;
        }
        if let Some(p) = env("COCKPIT_CONTAINER").filter(|s| !s.trim().is_empty()) {
            c.container_env = Some(runtime(&p)?);
        }
        if let Some(p) = env("COCKPIT_AGENT_IO").filter(|s| !s.trim().is_empty()) {
            let v = p.trim().to_ascii_lowercase();
            if !matches!(v.as_str(), "bind" | "copy") {
                return Err(ConfigError::InvalidValue(msg!(
                    ServerConfigAgentIoInvalid,
                    value = &p
                )));
            }
            c.agent_io = Some(v);
        }
        if let Some(p) = env("COCKPIT_DEV_PORT").filter(|s| !s.trim().is_empty()) {
            let nr: u16 = p.trim().parse().map_err(|_| {
                ConfigError::InvalidValue(msg!(ServerConfigDevPortInvalid, value = &p))
            })?;
            if nr == 0 {
                return Err(ConfigError::InvalidValue(msg!(ServerConfigDevPortZero)));
            }
            c.dev_port = Some(nr);
        }
        if let Some(b) = env("COCKPIT_BIND").filter(|s| !s.trim().is_empty()) {
            c.bind = b.trim().parse().map_err(|_| {
                ConfigError::InvalidValue(msg!(ServerConfigBindInvalid, value = &b))
            })?;
        }
        if let Some(p) = env("COCKPIT_PUBLIC_PORT").filter(|s| !s.trim().is_empty()) {
            let nr: u16 = p.trim().parse().ok().filter(|n| *n != 0).ok_or_else(|| {
                ConfigError::InvalidValue(msg!(ServerConfigPublicPortInvalid, value = &p))
            })?;
            c.public_port = Some(nr);
        }
        if let Some(p) = env("COCKPIT_PORT").filter(|s| !s.is_empty()) {
            c.port = p.trim().parse().map_err(|_| {
                ConfigError::InvalidValue(msg!(ServerConfigPortInvalid, value = &p))
            })?;
        }
        if let Some(p) = env("COCKPIT_DIST").filter(|s| !s.is_empty()) {
            c.dist = p.into();
        }
        if let Some(p) = env("COCKPIT_DB").filter(|s| !s.is_empty()) {
            c.db = p.into();
        }
        if c.port == 0 {
            return Err(ConfigError::InvalidValue(msg!(ServerConfigPortZero)));
        }
        Ok(c)
    }
}

/// Trims a list of names and drops the empty ones.
fn clean_list(list: Vec<String>) -> Vec<String> {
    list.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
