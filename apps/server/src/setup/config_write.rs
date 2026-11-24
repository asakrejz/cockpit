//! Editing config.toml text: apply a change and write the file atomically.

use super::*;

/// Sets `key = value` in the table and keeps comments of the line (before and after).
pub(super) fn set(table: &mut dyn toml_edit::TableLike, key: &str, updated: Item) {
    // Replace the existing entry in place: that way the key keeps its comment in front of it.
    let Some(alt) = table.get_mut(key) else {
        table.insert(key, updated);
        return;
    };
    let decor = alt.as_value().map(|v| v.decor().clone());
    *alt = updated;
    if let (Some(decor), Some(v)) = (decor, alt.as_value_mut()) {
        *v.decor_mut() = decor;
    }
}

/// The table `[name]`; it is created if it is missing.
pub(super) fn table<'a>(
    doc: &'a mut DocumentMut,
    name: &str,
) -> Result<&'a mut dyn toml_edit::TableLike, Msg> {
    if !doc.contains_key(name) {
        doc.insert(name, Item::Table(Table::new()));
    }
    doc.get_mut(name)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| msg!(ServerSetupTableInvalid, name = name))
}

/// Removes a table if it is empty.
pub(super) fn empty_remove(doc: &mut DocumentMut, name: &str) {
    let empty = doc
        .get(name)
        .and_then(Item::as_table_like)
        .is_some_and(|t| t.is_empty());
    if empty {
        doc.remove(name);
    }
}

pub(super) fn list(entries: &[String]) -> Item {
    value(entries.iter().map(String::as_str).collect::<Array>())
}

/// Writes provider and endpoint (`[agent] provider`, `[agent.endpoint]`).
pub(super) fn endpoint_write(
    doc: &mut DocumentMut,
    e: &cockpit_core::api::EndpointInput,
) -> Result<(), Msg> {
    let url = e.url.as_deref().map(endpoint_url_read).transpose()?;
    let model = match e.model.as_deref().map(str::trim) {
        Some(m) if m.chars().any(|c| c.is_whitespace() || c.is_control()) => {
            return Err(msg!(ServerSetupModelNameInvalid));
        }
        other => other,
    };
    {
        let agent = table(doc, "agent")?;
        if let Some(provider) = e.provider {
            set(agent, "provider", value(provider.id()));
        }
        if !agent.contains_key("endpoint") {
            agent.insert("endpoint", Item::Table(Table::new()));
        }
        let ep = agent
            .get_mut("endpoint")
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| msg!(ServerSetupTableInvalid, name = "agent.endpoint"))?;
        match url.as_deref() {
            Some("") => {
                ep.remove("url");
            }
            Some(u) => set(ep, "url", value(u)),
            None => {}
        }
        match model {
            Some("") => {
                ep.remove("model");
            }
            Some(m) => set(ep, "model", value(m)),
            None => {}
        }
        if ep.is_empty() {
            agent.remove("endpoint");
        }
    }
    empty_remove(doc, "agent");
    Ok(())
}

/// Applies the change to the text of `config.toml`. The text is then checked as at
/// start (`Config::parse`); an error means: write nothing.
pub fn change(text: &str, a: &ConfigChange) -> Result<(String, Config), Msg> {
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        msg!(ServerSetupTomlBroken, reason = Msg::raw(e.message()))
    })?;

    if let Some(j) = &a.jira {
        {
            let t = table(&mut doc, "jira")?;
            if let Some(site) = &j.site {
                let site = jira_site_read(site)?;
                if site.is_empty() {
                    t.remove("site");
                } else {
                    set(t, "site", value(site));
                }
            }
            if let Some(email) = &j.email {
                let email = email.trim();
                if email.is_empty() {
                    t.remove("email");
                } else if !email.contains('@') || email.contains(char::is_whitespace) {
                    return Err(msg!(ServerSetupEmailInvalid));
                } else {
                    set(t, "email", value(email));
                }
            }
            if let Some(p) = &j.projects {
                let projects = projects_read(&p.join(","));
                if projects.is_empty() {
                    t.remove("projects");
                } else {
                    set(t, "projects", list(&projects));
                }
            }
        }
        empty_remove(&mut doc, "jira");
    }
    if let Some(g) = &a.github_projects {
        {
            let t = table(&mut doc, "github_projects")?;
            if let Some(from) = g.tickets_from {
                set(t, "tickets_from", value(from.id()));
            }
            if let Some(list_in) = &g.projects {
                let mut projects: Vec<String> = Vec::new();
                for text in list_in {
                    let r = cockpit_core::ProjectRef::parse(text)
                        .ok_or_else(|| msg!(ServerConfigProjectForm, project = text.as_str()))?;
                    if !projects.contains(&r.to_string()) {
                        projects.push(r.to_string());
                    }
                }
                set(t, "projects", list(&projects));
            }
            for (key, input) in [
                ("status_field", &g.status_field),
                ("team_field", &g.team_field),
            ] {
                if let Some(name) = input {
                    let name = name.trim();
                    if name.is_empty() {
                        t.remove(key);
                    } else if name.chars().any(char::is_control) {
                        return Err(msg!(ServerSetupFieldNameInvalid));
                    } else {
                        set(t, key, value(name));
                    }
                }
            }
        }
    }
    for (name, input) in [
        ("atlassian", &a.atlassian),
        ("github_oauth", &a.github_oauth),
    ] {
        if let Some(c) = input {
            let id = client_id_read(&c.client_id)?;
            if id.is_empty() {
                doc.remove(name);
            } else {
                set(table(&mut doc, name)?, "client_id", value(id));
            }
        }
    }
    if let Some(site) = &a.atlassian_site {
        let site = site.trim();
        if doc.contains_key("atlassian") {
            let t = table(&mut doc, "atlassian")?;
            if site.is_empty() {
                t.remove("site");
            } else if site.chars().any(char::is_control) {
                return Err(msg!(ServerSetupSiteNameInvalid));
            } else {
                set(t, "site", value(site));
            }
        } else if !site.is_empty() {
            return Err(msg!(ServerSetupSiteNeedsClientId));
        }
    }
    if let Some(g) = &a.github {
        let repos = repos_read(&g.repos.join(","))?;
        {
            let t = table(&mut doc, "github")?;
            if repos.is_empty() {
                t.remove("repos");
            } else {
                set(t, "repos", list(&repos));
            }
        }
        empty_remove(&mut doc, "github");
        // An entry of `[[agent.repos]]` that is no longer in the list is not watched any more
        // (it keeps its settings).
        if let Some(Item::ArrayOfTables(all)) = doc
            .get_mut("agent")
            .and_then(Item::as_table_like_mut)
            .and_then(|a| a.get_mut("repos"))
        {
            for e in all.iter_mut() {
                let name = e.get("repo").and_then(Item::as_str).unwrap_or_default();
                if repos.iter().any(|r| r == name) {
                    e.remove("watch");
                } else {
                    e.insert("watch", value(false));
                }
            }
        }
    }
    for (name, choice) in [("github", a.github_auth), ("jira", a.jira_auth)] {
        if let Some(c) = choice {
            set(table(&mut doc, name)?, "auth", value(c.id()));
        }
    }
    if let Some(r) = &a.agent_repos {
        agent_repos_write(&mut doc, r)?;
    }
    if let Some(e) = &a.endpoint {
        endpoint_write(&mut doc, e)?;
    }
    if let Some(w) = &a.webhooks {
        {
            let t = table(&mut doc, "server")?;
            if let Some(on) = w.enabled {
                set(t, "webhooks", value(on));
            }
            if let Some(url) = &w.public_url {
                let url = cockpit_core::setup::public_url_read(url)?;
                if url.is_empty() {
                    t.remove("public_url");
                } else {
                    set(t, "public_url", value(url));
                }
            }
        }
        empty_remove(&mut doc, "server");
    }

    repo_projects_drop(&mut doc);
    let updated = doc.to_string();
    let config = Config::parse(Some(&updated), |_| None).map_err(|e| e.msg())?;
    Ok((updated, config))
}

/// The key `projects` of an `[[agent.repos]]` entry has no effect (the file is still read with
/// it); every save leaves it out.
fn repo_projects_drop(doc: &mut DocumentMut) {
    if let Some(all) = doc
        .get_mut("agent")
        .and_then(Item::as_table_like_mut)
        .and_then(|a| a.get_mut("repos"))
        .and_then(Item::as_array_of_tables_mut)
    {
        for entry in all.iter_mut() {
            entry.remove("projects");
        }
    }
}

/// Writes `[agent] repo` (the default) and `[[agent.repos]]` (the list with base branch, branch
/// pattern and language). An empty list removes both. Other values of `[agent]` stay.
pub(super) fn agent_repos_write(doc: &mut DocumentMut, input: &AgentReposInput) -> Result<(), Msg> {
    if let Some(d) = &input.default
        && !input.repos.iter().any(|r| &r.repo == d)
    {
        return Err(msg!(ServerSetupDefaultRepoMissing, repo = d));
    }
    let t = table(doc, "agent")?;
    t.remove("repos");
    // `[github] repos` (the watched repos) is a separate list and stays as it is.
    let t = table(doc, "agent")?;
    match input
        .default
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        Some(d) => set(t, "repo", value(d)),
        None => {
            t.remove("repo");
        }
    }
    if !input.repos.is_empty() {
        let mut all = toml_edit::ArrayOfTables::new();
        for r in &input.repos {
            let mut e = Table::new();
            e.insert("repo", value(r.repo.trim()));
            if r.base_branch != cockpit_core::repos::DEFAULT_BASE {
                e.insert("base_branch", value(r.base_branch.trim()));
            }
            let pattern = r.branch_pattern.trim();
            if !pattern.is_empty() && pattern != cockpit_core::branch::DEFAULT_PATTERN {
                e.insert("branch_pattern", value(pattern));
            }
            if r.pr_language != cockpit_i18n::Lang::En {
                e.insert("pr_language", value(r.pr_language.code()));
            }
            if !r.watch {
                e.insert("watch", value(false));
            }
            all.push(e);
        }
        t.insert("repos", Item::ArrayOfTables(all));
    }
    empty_remove(doc, "agent");
    Ok(())
}

/// Writes the file atomically: first a neighbor file, then rename. An existing file
/// keeps its permissions; a new one is readable only by the user.
pub fn atomic_write(path: &Path, text: &str) -> std::io::Result<()> {
    // A link to the file stays a link: we replace the target.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".into());
    let tmp = dir.join(format!(".{name}.neu-{}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        if let Ok(alt) = std::fs::metadata(&path) {
            std::fs::set_permissions(&tmp, alt.permissions())?;
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            }
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
