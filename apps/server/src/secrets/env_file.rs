//! `EnvFileStore`: the credentials live in a `.env` file in the folder `config/` of the project.
//!
//! File format (one entry per line):
//!
//! ```text
//! # comment
//! JIRA_TOKEN=plain-value
//! GITHUB_OAUTH_TOKENS='{"access":"…","refresh":"…"}'
//! AGENT_KEY="value with ' and \" and \n escapes"
//! ```
//!
//! * Values that need quoting are written in single quotes (literal, nothing is escaped). Only if
//!   the value contains a `'` or a line break it is written in double quotes with the escapes
//!   `\\`, `\"`, `\n`, `\r`.
//! * The OAuth token blobs are compact single-line JSON (what the backend produces anyway), so
//!   they stay on one line in single quotes. No base64.
//! * A process environment variable of the same name wins over the file (usual for deployments).
//! * Writing is atomic (temp file in the same folder, then rename), mode `600`; comments and
//!   foreign lines survive; an existing line is replaced, never appended twice.
//! * Values never appear in `Debug`, error messages or logs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cockpit_integrations::Secret;

use super::{
    AGENT_KEY, ATLASSIAN_OAUTH, ATLASSIAN_OAUTH_TOKENS, GITHUB, GITHUB_OAUTH, GITHUB_OAUTH_TOKENS,
    GITHUB_WEBHOOK, JIRA, JIRA_WEBHOOK, SecretError, SecretStore,
};

/// Variable that points the server to a different file (tests, operation).
pub const ENV_FILE_VAR: &str = "COCKPIT_ENV_FILE";
/// Default file name, inside the folder `config/` of the project (next to `config.toml` and
/// `cockpit.db`).
pub const DEFAULT_FILE_NAME: &str = ".env";

/// Every secret name and the environment variable it lives in.
pub const VARIABLES: &[(&str, &str)] = &[
    (GITHUB, "GITHUB_TOKEN"),
    (JIRA, "JIRA_TOKEN"),
    (GITHUB_OAUTH, "GITHUB_OAUTH_CLIENT_SECRET"),
    (ATLASSIAN_OAUTH, "ATLASSIAN_OAUTH_CLIENT_SECRET"),
    (AGENT_KEY, "AGENT_KEY"),
    (GITHUB_OAUTH_TOKENS, "GITHUB_OAUTH_TOKENS"),
    (ATLASSIAN_OAUTH_TOKENS, "ATLASSIAN_OAUTH_TOKENS"),
    (GITHUB_WEBHOOK, "GITHUB_WEBHOOK_SECRET"),
    (JIRA_WEBHOOK, "JIRA_WEBHOOK_SECRET"),
];

/// The variable for a secret name.
pub fn variable_for(name: &str) -> Option<&'static str> {
    VARIABLES.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

type EnvLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct EnvFileStore {
    path: PathBuf,
    env: EnvLookup,
    /// Serialises read-modify-write cycles.
    write_lock: Mutex<()>,
}

impl std::fmt::Debug for EnvFileStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Only the path, never a value.
        f.debug_struct("EnvFileStore")
            .field("path", &self.path)
            .finish()
    }
}

impl EnvFileStore {
    /// File at `path`, process environment from the real environment.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_env(path, |k| std::env::var(k).ok())
    }

    /// Like [`new`](Self::new) with a replaceable environment lookup (tests).
    pub fn with_env(
        path: impl Into<PathBuf>,
        env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            path: path.into(),
            env: Arc::new(env),
            write_lock: Mutex::new(()),
        }
    }

    /// `COCKPIT_ENV_FILE` if set, else `config/.env` in the project folder.
    pub fn from_environment() -> Self {
        Self::new(default_path(|k| std::env::var(k).ok()))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// For the server start: tightens the permissions to 600 and returns warnings (tracked by
    /// Git, not ignored, was too open).
    pub fn check_at_startup(&self) -> Vec<String> {
        super::env_check::check_and_fix(&self.path)
    }

    fn variable(name: &str) -> Result<&'static str, SecretError> {
        variable_for(name).ok_or_else(|| SecretError(format!("Unknown name \"{name}\"")))
    }

    fn read_text(&self) -> Result<String, SecretError> {
        match std::fs::read_to_string(&self.path) {
            Ok(t) => Ok(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(SecretError(format!(
                "File {} not readable ({})",
                self.path.display(),
                e.kind()
            ))),
        }
    }

    /// Writes `text` atomically with mode 600.
    fn write_text(&self, text: &str) -> Result<(), SecretError> {
        let fail = |what: &str, e: std::io::Error| {
            SecretError(format!(
                "File {} not writable ({what}: {})",
                self.path.display(),
                e.kind()
            ))
        };
        // Follow a symlink, so the link itself is kept.
        let target = std::fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        let dir = target
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| fail("Ordner", e))?;
        let tmp = dir.join(format!(
            ".env.tmp-{}-{}",
            std::process::id(),
            next_counter()
        ));
        let result = (|| {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut file = opts.open(&tmp).map_err(|e| fail("anlegen", e))?;
            file.write_all(text.as_bytes())
                .map_err(|e| fail("schreiben", e))?;
            file.sync_all().map_err(|e| fail("sichern", e))?;
            drop(file);
            std::fs::rename(&tmp, &target).map_err(|e| fail("ersetzen", e))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
}

fn next_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Where the file lives: `COCKPIT_ENV_FILE`, else `config/.env` in the project folder.
pub fn default_path(env: impl Fn(&str) -> Option<String>) -> PathBuf {
    match env(ENV_FILE_VAR).filter(|s| !s.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => crate::config::location::default_env_path(&crate::config::location::project_dir()),
    }
}

impl SecretStore for EnvFileStore {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretError> {
        let var = Self::variable(name)?;
        if let Some(v) = (self.env)(var).filter(|v| !v.is_empty()) {
            return Ok(Some(Secret::new(v)));
        }
        let text = self.read_text()?;
        Ok(lookup(&text, var)
            .filter(|v| !v.is_empty())
            .map(Secret::new))
    }

    fn set(&self, name: &str, value: &Secret) -> Result<(), SecretError> {
        let var = Self::variable(name)?;
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let text = replace(&self.read_text()?, var, Some(value.expose()));
        self.write_text(&text)
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        let var = Self::variable(name)?;
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let old = self.read_text()?;
        let new = replace(&old, var, None);
        if new == old {
            return Ok(());
        }
        self.write_text(&new)
    }

    fn env_file(&self) -> Option<&Path> {
        Some(&self.path)
    }
}

/// Name and raw value of an assignment line, `None` for comments, blanks, foreign text.
fn parse_line(line: &str) -> Option<(&str, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").map_or(line, str::trim_start);
    let (name, rest) = line.split_once('=')?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some((name, parse_value(rest.trim())))
}

fn parse_value(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix('\'') {
        if let Some(end) = rest.find('\'') {
            return rest[..end].to_string();
        }
    } else if let Some(rest) = raw.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return out,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some(c @ ('"' | '\\' | '$')) => out.push(c),
                    Some(c) => {
                        out.push('\\');
                        out.push(c);
                    }
                    None => out.push('\\'),
                },
                c => out.push(c),
            }
        }
    }
    // Unquoted: up to an inline comment.
    let end = raw
        .find(" #")
        .or_else(|| raw.find("\t#"))
        .unwrap_or(raw.len());
    raw[..end].trim_end().to_string()
}

/// The value of the last assignment of `var`.
fn lookup(text: &str, var: &str) -> Option<String> {
    text.lines()
        .filter_map(parse_line)
        .filter(|(n, _)| *n == var)
        .map(|(_, v)| v)
        .next_back()
}

/// The line for `var=value`, quoted where needed.
fn format_line(var: &str, value: &str) -> String {
    let safe = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:/@+,%-".contains(c));
    if safe {
        format!("{var}={value}")
    } else if !value.contains(['\'', '\n', '\r']) {
        format!("{var}='{value}'")
    } else {
        let mut quoted = String::new();
        for c in value.chars() {
            match c {
                '\\' => quoted.push_str("\\\\"),
                '"' => quoted.push_str("\\\""),
                '\n' => quoted.push_str("\\n"),
                '\r' => quoted.push_str("\\r"),
                c => quoted.push(c),
            }
        }
        format!("{var}=\"{quoted}\"")
    }
}

/// Replaces (`Some`) or removes (`None`) the assignment of `var`; everything else stays as it is.
fn replace(text: &str, var: &str, value: Option<&str>) -> String {
    let new_line = value.map(|v| format_line(var, v));
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    // `split('\n')` keeps the final empty element, so the trailing newline survives the join.
    for line in text.split('\n') {
        if parse_line(line).is_some_and(|(n, _)| n == var) {
            if let (Some(l), false) = (&new_line, replaced) {
                out.push(l.clone());
                replaced = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    let mut result = out.join("\n");
    if let (Some(l), false) = (new_line, replaced) {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&l);
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests;
