//! Git on the user's machine for the agent: cloning before the run, checking and pushing after it.
//!
//! The container gets **no** GitHub token. The backend clones (fresh, per run), the container
//! commits to a branch, and the backend only pushes after its check:
//!
//! * only a branch that the configured pattern of the repo produces (`claude/{ticket}-{slug}` by
//!   default, see `cockpit_core::branch`) – never the main branch, the base branch, a protected
//!   name or a branch that already exists on the remote, never a force push, never a merge,
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

use cockpit_i18n::{Msg, msg};
use cockpit_integrations::Secret;

/// A base branch to branch off: inconspicuous characters, no leading `-` (no option for `git`).
pub fn base_branch_ok(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('-')
        && !branch.contains("..")
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

/// The branch of a run and what it is checked against: the pattern of the repo (from the
/// configuration, never from the request) and the base branch.
#[derive(Clone, Copy, Debug)]
pub struct BranchRule<'a> {
    pub branch: &'a str,
    pub pattern: &'a str,
    pub base: &'a str,
    /// A follow-up on the branch of an existing pull request. Otherwise the branch is new and
    /// must not exist on the remote yet.
    pub existing: bool,
}

impl BranchRule<'_> {
    /// Valid name, not protected, produced by the pattern. A follow-up may also keep a name
    /// from the default pattern.
    pub fn check(&self) -> Result<(), Msg> {
        if self.existing {
            cockpit_core::branch::check_existing(self.pattern, self.base, self.branch)
        } else {
            cockpit_core::branch::check(self.pattern, self.base, self.branch)
        }
    }
}

/// A branch name of a pull request that is safe to hand to `git clone --branch`: no leading `-`,
/// no `..`, only plain characters. Unlike [`branch_ok`] it is not limited to `claude/…`, because a
/// review run only reads.
pub fn review_branch_ok(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with(['-', '/', '.'])
        && branch.len() <= 200
        && !branch.contains("..")
        && !branch.contains("//")
        && !branch.ends_with(['/', '.'])
        && !branch.ends_with(".lock")
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
) -> Result<String, Msg> {
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
        .map_err(|e| msg!(ServerRunGitNotStarted, error = e.to_string()))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let error = String::from_utf8_lossy(&out.stderr);
    let error = error.trim();
    let secret = token.map(Secret::expose).unwrap_or("");
    Err(if error.is_empty() {
        msg!(ServerRunGitNoMessage)
    } else {
        Msg::raw(redact(error, &[secret]))
    })
}

/// A new branch must not exist on the remote: an existing one is somebody else's work (or
/// protected), and the agent never pushes onto it.
fn branch_is_new(url: &str, token: Option<&Secret>, branch: &str) -> Result<(), Msg> {
    let out = git(
        &[
            "ls-remote",
            "--heads",
            "--",
            url,
            &format!("refs/heads/{branch}"),
        ],
        None,
        url,
        token,
    )?;
    if out.trim().is_empty() {
        Ok(())
    } else {
        Err(msg!(ServerRunBranchExists, branch = branch))
    }
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

mod clone;
pub use clone::*;

mod publish;
pub use publish::*;

#[cfg(test)]
mod tests;
