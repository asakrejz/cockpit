//! Parsing and checking what the user typed (names, URLs, lists).

use cockpit_i18n::{Msg, msg};

use super::*;

/// Checks a key before sending. Returns the cleaned value (whitespace at the edges
/// removed, e.g. from pasting). The messages never name the value.
pub fn key_verify(value: &str) -> Result<String, Msg> {
    let value = value.trim();
    if value.is_empty() {
        return Err(msg!(CoreSetupKeyEmpty));
    }
    if value.chars().count() > MAX_KEYS {
        return Err(msg!(CoreSetupKeyTooLong));
    }
    if value.chars().any(char::is_control) {
        return Err(msg!(CoreSetupKeyControl));
    }
    Ok(value.to_string())
}

/// Checks the public address of the tunnel (`[server] public_url`): `https://host[:port][/path]`,
/// no spaces, no user name, query or fragment. Returns it without a trailing slash; empty
/// stays empty (= remove). The address is only displayed, the server never calls it.
pub fn public_url_read(text: &str) -> Result<String, Msg> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    let rest = text
        .strip_prefix("https://")
        .ok_or_else(|| msg!(CoreSetupPublicUrlHttps))?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty()
        || text.len() > 200
        || text.chars().any(|c| c.is_whitespace() || c.is_control())
        || text.contains(['?', '#', '@', '\\'])
    {
        return Err(msg!(CoreSetupPublicUrlInvalid));
    }
    Ok(text.trim_end_matches('/').to_string())
}

/// The site of a Jira Cloud instance as an address: `firma` → `https://firma.atlassian.net`,
/// `firma.atlassian.net` and `https://firma.atlassian.net/jira/…` → `https://firma.atlassian.net`.
/// Empty stays empty.
pub fn jira_site_read(text: &str) -> Result<String, Msg> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    if t.chars().any(char::is_whitespace) {
        return Err(msg!(CoreSetupAddressSpaces));
    }
    // The own machine (for tests with a dummy) may be addressed without https.
    if let Some((s, r)) = t.split_once("://")
        && s.eq_ignore_ascii_case("http")
    {
        let host = r.split(['/', '?', '#']).next().unwrap_or_default();
        let name = host.split(':').next().unwrap_or_default();
        let port_ok = host
            .split_once(':')
            .is_none_or(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        if matches!(name, "127.0.0.1" | "localhost") && port_ok {
            return Ok(format!("http://{host}"));
        }
    }
    let (scheme, rest) = match t.split_once("://") {
        Some((s, r)) if s.eq_ignore_ascii_case("https") => ("https", r),
        Some((s, _)) if s.eq_ignore_ascii_case("http") => {
            return Err(msg!(CoreSetupJiraHttpsOnly));
        }
        Some(_) => return Err(msg!(CoreSetupJiraHttpsStart)),
        None => ("https", t),
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        return Err(msg!(CoreSetupJiraNoSite));
    }
    let host = host.to_ascii_lowercase();
    let host = if host.contains('.') {
        host
    } else {
        format!("{host}.atlassian.net")
    };
    let valid = host
        .split('.')
        .all(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    if !valid {
        return Err(msg!(CoreSetupJiraBadHost, host = host));
    }
    Ok(format!("{scheme}://{host}"))
}

/// Jira project keys from a text ("bau, web" or one line per project), uppercased
/// and without duplicates.
pub fn projects_read(text: &str) -> Vec<String> {
    let mut from: Vec<String> = Vec::new();
    for part in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let p = part.trim().to_uppercase();
        if !p.is_empty() && !from.contains(&p) {
            from.push(p);
        }
    }
    from
}

/// The agent's repos from the rows of the assistant: `(repo, base branch, projects)` per row
/// plus the default. Empty rows are skipped. Rejects (with a plain-text reason) a repo that
/// is not `owner/name`, one that is listed twice, a base branch with odd characters, a Jira
/// project that is not a key or that goes to two repos, and a default outside the list.
pub fn agent_repos_read(
    rows: &[(String, String, String, String)],
    default: &str,
) -> Result<crate::api::AgentReposInput, Msg> {
    let mut repos: Vec<crate::AgentRepo> = Vec::new();
    for (repo, base, projects, pattern) in rows {
        if repo.trim().is_empty() {
            continue;
        }
        let name = match repos_read(repo)?.as_slice() {
            [one] => one.clone(),
            _ => return Err(msg!(CoreSetupRepoOnePerLine, repo = repo.trim())),
        };
        if repos.iter().any(|r| r.repo == name) {
            return Err(msg!(CoreSetupRepoTwice, name = &name));
        }
        let base = base.trim();
        let base = if base.is_empty() {
            crate::repos::DEFAULT_BASE
        } else {
            base
        };
        if base.starts_with('-')
            || base.contains("..")
            || !base
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        {
            return Err(msg!(CoreSetupBaseInvalid, base = base, name = &name));
        }
        let projects = projects_read(projects);
        for p in &projects {
            if !crate::model::is_project_key(p) {
                return Err(msg!(CoreSetupNotProjectKey, project = p));
            }
            if let Some(other) = repos.iter().find(|r| r.projects.contains(p)) {
                return Err(msg!(CoreSetupProjectTaken, project = p, repo = &other.repo));
            }
        }
        let pattern = pattern.trim();
        let pattern = if pattern.is_empty() {
            crate::branch::DEFAULT_PATTERN
        } else {
            pattern
        };
        crate::branch::validate(pattern)
            .map_err(|m| msg!(CoreSetupRepoPattern, name = &name, message = m))?;
        repos.push(crate::AgentRepo {
            repo: name,
            base_branch: base.to_string(),
            projects,
            branch_pattern: pattern.to_string(),
        });
    }
    let default = default.trim();
    let default = if default.is_empty() {
        None
    } else {
        let d = repos_read(default)?.into_iter().next().unwrap_or_default();
        if !repos.iter().any(|r| r.repo == d) {
            return Err(msg!(CoreSetupDefaultRepoMissing, name = &d));
        }
        Some(d)
    };
    Ok(crate::api::AgentReposInput { repos, default })
}

/// GitHub repos from a text. Allows `owner/name`, full addresses (`https://github.com/o/n`,
/// also with `.git`) and separation by comma, space or lines.
pub fn repos_read(text: &str) -> Result<Vec<String>, Msg> {
    let mut from: Vec<String> = Vec::new();
    for part in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let mut r = part.trim();
        if r.is_empty() {
            continue;
        }
        for prefix in ["https://github.com/", "http://github.com/", "github.com/"] {
            if let Some(rest) = r.strip_prefix(prefix) {
                r = rest;
                break;
            }
        }
        let r = r.trim_end_matches('/');
        let r = r.strip_suffix(".git").unwrap_or(r);
        // For a full address only the first two parts count (`…/o/n/pull/3`).
        let mut parts = r.split('/');
        let (Some(o), Some(n)) = (parts.next(), parts.next()) else {
            return Err(msg!(CoreSetupNotRepo, part = part));
        };
        let chars = |t: &str| {
            !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        };
        if !chars(o) || !chars(n) {
            return Err(msg!(CoreSetupNotRepo, part = part));
        }
        let repo = format!("{o}/{n}");
        if !from.contains(&repo) {
            from.push(repo);
        }
    }
    Ok(from)
}

/// A client ID from the input field: without whitespace at the edges, without spaces inside.
pub fn client_id_read(text: &str) -> Result<String, Msg> {
    let t = text.trim();
    if t.chars().any(char::is_whitespace) {
        return Err(msg!(CoreSetupClientIdSpaces));
    }
    Ok(t.to_string())
}

/// The address of the endpoint (LiteLLM, another compatible service): without `http://` or
/// `https://`, `http://` is added (`localhost:4000` → `http://localhost:4000`), a
/// trailing slash is dropped. Empty stays empty (= directly at the provider).
pub fn endpoint_url_read(text: &str) -> Result<String, Msg> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(String::new());
    }
    if t.chars().any(char::is_whitespace) {
        return Err(msg!(CoreSetupAddressSpaces));
    }
    let url = if t.contains("://") {
        t.to_string()
    } else {
        format!("http://{t}")
    };
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or_else(|| msg!(CoreSetupUrlScheme))?;
    if rest.trim_matches('/').is_empty() {
        return Err(msg!(CoreSetupUrlNoHost));
    }
    Ok(url.trim_end_matches('/').to_string())
}

/// Checks a model name typed by hand: not empty, without whitespace and control characters, not too long.
pub fn model_name_read(text: &str) -> Result<String, Msg> {
    let t = text.trim();
    if t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(msg!(CoreSetupModelNameSpaces));
    }
    if t.chars().count() > 200 {
        return Err(msg!(CoreSetupModelNameLong));
    }
    Ok(t.to_string())
}
