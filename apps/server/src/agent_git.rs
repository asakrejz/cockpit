//! Git on the user's machine for the agent: cloning before the run, checking and pushing after it.
//!
//! The container gets **no** GitHub token. The backend clones (fresh, per run), the container
//! commits to a branch `claude/…`, and the backend only pushes after its check:
//!
//! * only branches with the prefix `claude/` – never the main branch, never a force push, never a merge,
//! * the branch must build on the state from which it was cloned,
//! * the changed files must fit the rules of the repo.
//!
//! After the run the clone is **not trustworthy** (the container was allowed to write into it, also
//! into `.git`). Therefore no Git runs in the clone itself after the run: the branch is fetched into a fresh,
//! empty bare repo (`git fetch <clone>`), and only there is it checked and pushed. The
//! credentials only go through the environment of the child process (`GIT_CONFIG_*`), never through the
//! command line, and only to `https://` addresses.

use std::path::Path;
use std::process::{Command, Stdio};

use cockpit_core::RepoRules;
use cockpit_integrations::Secret;

/// This many changed files the backend lets through.
const MAX_FILES: usize = 500;

/// `owner/name`, as GitHub knows it.
pub fn repo_name_ok(repo: &str) -> bool {
    let teil = |t: &str| {
        !t.is_empty()
            && t != "."
            && t != ".."
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    repo.split_once('/')
        .is_some_and(|(o, n)| teil(o) && teil(n))
}

/// Only branches `claude/…` with inconspicuous characters. The main branch is thereby excluded.
pub fn branch_ok(branch: &str) -> bool {
    branch.starts_with("claude/")
        && branch.len() <= 120
        && !branch.contains("..")
        && !branch.contains("//")
        && !branch.ends_with('/')
        && !branch.ends_with(".lock")
        && !branch.ends_with('.')
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

/// The address for cloning: `<base>/<owner>/<name>.git`.
pub fn clone_url(base: &str, repo: &str) -> String {
    format!("{}/{repo}.git", base.trim_end_matches('/'))
}

/// The web address from the API address: `https://api.github.com` → `https://github.com`,
/// `https://ghe.example/api/v3` → `https://ghe.example`.
pub fn base_from_api(api_url: &str) -> String {
    let url = api_url.trim().trim_end_matches('/');
    if url == "https://api.github.com" {
        return "https://github.com".into();
    }
    url.strip_suffix("/api/v3").unwrap_or(url).to_string()
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Environment variables for the sign-in via HTTPS (`http.extraHeader`). Empty for everything that is not
/// `https://`: the token never goes over plain-text connections or to local paths.
fn auth_env(url: &str, token: Option<&Secret>) -> Vec<(String, String)> {
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return Vec::new();
    };
    if !url.starts_with("https://") {
        return Vec::new();
    }
    let basic = base64(format!("x-access-token:{}", token.expose()).as_bytes());
    vec![
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        ("GIT_CONFIG_KEY_0".into(), "http.extraHeader".into()),
        (
            "GIT_CONFIG_VALUE_0".into(),
            format!("Authorization: Basic {basic}"),
        ),
    ]
}

/// Removes credentials from a text (error messages of Git can contain the address).
pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut t = text.to_string();
    for g in secrets.iter().filter(|g| g.len() >= 4) {
        t = t.replace(g, "***");
        t = t.replace(&base64(format!("x-access-token:{g}").as_bytes()), "***");
    }
    t
}

fn git(
    args: &[&str],
    cwd: Option<&Path>,
    url_for_auth: &str,
    token: Option<&Secret>,
) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        // No hooks, no schedule, no helper programs from the clone.
        .env("GIT_CONFIG_NOSYSTEM", "1");
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    for (k, v) in auth_env(url_for_auth, token) {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("git lässt sich nicht starten: {e}. Ist Git installiert?"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let error = String::from_utf8_lossy(&out.stderr);
    let error = error.trim();
    let secret = token.map(Secret::expose).unwrap_or("");
    Err(redact(
        if error.is_empty() {
            "ohne Fehlermeldung"
        } else {
            error
        },
        &[secret],
    ))
}

/// Opens permissions in the clone so that the user in the container (different id, with Podman rootless
/// a different mapping) can write and the backend can delete afterwards. The folder is in a
/// directory that belongs only to the user.
#[cfg(unix)]
pub fn permissions_open(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fn rec(p: &Path) {
        let Ok(meta) = std::fs::symlink_metadata(p) else {
            return;
        };
        if meta.file_type().is_symlink() {
            return;
        }
        let mode = if meta.is_dir() { 0o777 } else { 0o666 };
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
        if meta.is_dir()
            && let Ok(children) = std::fs::read_dir(p)
        {
            for k in children.flatten() {
                rec(&k.path());
            }
        }
    }
    rec(dir);
}

#[cfg(not(unix))]
pub fn permissions_open(_dir: &Path) {}

/// Clones `url` to `target`, creates the branch and returns the state from which it starts.
/// The remote is removed afterwards: there is no address and no token in the clone.
pub fn clone(
    url: &str,
    target: &Path,
    token: Option<&Secret>,
    base_branch: &str,
    branch: &str,
) -> Result<String, String> {
    if !branch_ok(branch) {
        return Err(format!(
            "Der Branch „{branch}“ ist nicht erlaubt – nur claude/… wird angelegt."
        ));
    }
    if base_branch.is_empty()
        || base_branch.starts_with('-')
        || !base_branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
    {
        return Err(format!("Der Basis-Branch „{base_branch}“ ist ungültig."));
    }
    let z = target.to_string_lossy().into_owned();
    git(
        &[
            "clone",
            "--quiet",
            "--no-tags",
            "--depth",
            "50",
            "--single-branch",
            "--branch",
            base_branch,
            "--",
            url,
            &z,
        ],
        None,
        url,
        token,
    )?;
    git(
        &["switch", "--quiet", "-c", branch],
        Some(target),
        url,
        None,
    )?;
    let base = git(&["rev-parse", "HEAD"], Some(target), url, None)?
        .trim()
        .to_string();
    git(&["remote", "remove", "origin"], Some(target), url, None)?;
    permissions_open(target);
    Ok(base)
}

/// Clones the **existing** branch `branch` (a branch `claude/…` of a pull request) for a follow-up
/// job and returns the commit it stands on. New commits of the agent build on it; the backend
/// pushes them as a normal push (never a force-push). Like [`clone`], the clone has no remote and
/// no token afterwards.
pub fn clone_branch(
    url: &str,
    target: &Path,
    token: Option<&Secret>,
    branch: &str,
) -> Result<String, String> {
    if !branch_ok(branch) {
        return Err(format!(
            "Der Branch „{branch}“ ist nicht erlaubt – der Agent arbeitet nur auf claude/…."
        ));
    }
    let z = target.to_string_lossy().into_owned();
    git(
        &[
            "clone",
            "--quiet",
            "--no-tags",
            "--depth",
            "50",
            "--single-branch",
            "--branch",
            branch,
            "--",
            url,
            &z,
        ],
        None,
        url,
        token,
    )?;
    let base = git(&["rev-parse", "HEAD"], Some(target), url, None)?
        .trim()
        .to_string();
    git(&["remote", "remove", "origin"], Some(target), url, None)?;
    permissions_open(target);
    Ok(base)
}

/// What was pushed after the check.
#[derive(Debug, PartialEq, Eq)]
pub struct Published {
    pub commit: String,
    pub files: Vec<String>,
}

/// Checks the branch from the clone and pushes it. See the module docs for the order and why
/// nothing is ever worked on in the clone itself. `export` is an empty folder for the bare repo.
pub fn publish(
    clone: &Path,
    export: &Path,
    branch: &str,
    base_sha: &str,
    url: &str,
    token: Option<&Secret>,
    rules: &RepoRules,
) -> Result<Published, String> {
    if !branch_ok(branch) {
        return Err(format!(
            "Der Branch „{branch}“ ist nicht erlaubt – gepusht wird nur claude/…."
        ));
    }
    if base_sha.len() < 7 || !base_sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Der Ausgangsstand des Laufs ist unbekannt.".into());
    }
    let e = export.to_string_lossy().into_owned();
    git(&["init", "--quiet", "--bare", &e], None, "", None)?;
    let k = clone.to_string_lossy().into_owned();
    let spec = format!("+refs/heads/{branch}:refs/heads/{branch}");
    git(
        &["fetch", "--quiet", "--no-tags", "--", &k, &spec],
        Some(export),
        "",
        None,
    )
    .map_err(|m| format!("Der Branch {branch} fehlt im Klon des Agenten ({m})."))?;
    let ref_ = format!("refs/heads/{branch}");
    let commit = git(&["rev-parse", "--verify", &ref_], Some(export), "", None)?
        .trim()
        .to_string();
    // The branch must build on the starting state (no rewritten history, no foreign one).
    git(
        &["merge-base", "--is-ancestor", base_sha, &commit],
        Some(export),
        "",
        None,
    )
    .map_err(|_| {
        "Der Branch des Agenten baut nicht auf dem Stand auf, von dem aus geklont wurde."
            .to_string()
    })?;
    let count = git(
        &["rev-list", "--count", &format!("{base_sha}..{commit}")],
        Some(export),
        "",
        None,
    )?;
    if count.trim() == "0" {
        return Err("Der Agent hat nichts committet, es gibt nichts zu pushen.".into());
    }
    let names = git(
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            base_sha,
            &commit,
        ],
        Some(export),
        "",
        None,
    )?;
    let files: Vec<String> = names
        .split('\0')
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect();
    if files.is_empty() {
        return Err("Der Agent hat nichts geändert, es gibt nichts zu pushen.".into());
    }
    if files.len() > MAX_FILES {
        return Err(format!(
            "Der Agent hat {} Dateien geändert – mehr als {MAX_FILES} pusht das Cockpit nicht.",
            files.len()
        ));
    }
    let violations = rules.violations(&files);
    if !violations.is_empty() {
        let reasons: Vec<String> = violations.iter().map(|v| v.reason()).collect();
        return Err(format!(
            "Die Änderungen verstoßen gegen die Regeln des Repos und werden nicht gepusht: {}.",
            reasons.join("; ")
        ));
    }
    git(
        &["push", "--quiet", "--", url, &format!("{ref_}:{ref_}")],
        Some(export),
        url,
        token,
    )
    .map_err(|m| format!("Das Pushen auf {branch} ist fehlgeschlagen: {m}"))?;
    Ok(Published { commit, files })
}

#[cfg(test)]
mod tests;
