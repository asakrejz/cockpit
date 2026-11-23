//! Safety net around the folder `config/`: its files must not be tracked by Git (only
//! `config.example.toml` may be), must be covered by `.gitignore` and must not be readable by
//! others. Counterpart of `secrets/env_check`; nothing here reads a file's content.
//!
//! Without `git` or outside a repository the Git checks are skipped silently.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cockpit_i18n::{Msg, msg};

use super::location::EXAMPLE_FILE;

/// How long `/api/health` reuses a result (the check starts `git` a few times).
const CACHE_TTL: Duration = Duration::from_secs(15);

/// What `/api/health` tells about the folder: where it is and what is wrong.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ConfigFilesStatus {
    pub path: String,
    /// German text of the warnings (for readers that only know plain text).
    pub warnings: Vec<String>,
    /// The same warnings as code and parameters; the interface shows them in its language.
    pub warning_msgs: Vec<Msg>,
}

/// One finding of the check; it is a message for the interface or an English line for the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Finding {
    Loose { path: String, mode: u32 },
    Tracked { shown: String, name: String },
    NotIgnored { shown: String },
}

impl Finding {
    fn msg(&self) -> Msg {
        match self {
            Finding::Loose { path, mode } => {
                msg!(
                    ServerConfigFilesLoose,
                    path = path,
                    mode = format!("{mode:o}")
                )
            }
            Finding::Tracked { shown, name } => {
                msg!(ServerConfigFilesTracked, file = shown, name = name)
            }
            Finding::NotIgnored { shown } => msg!(ServerConfigFilesNotIgnored, file = shown),
        }
    }

    fn english(&self) -> String {
        match self {
            Finding::Loose { path, mode } => format!(
                "{path} is accessible by other users (mode {mode:o}); it should belong to you alone."
            ),
            Finding::Tracked { shown, name } => format!(
                "File {shown} is tracked by Git. A commit would publish your configuration. Remove it with `git rm --cached config/{name}` and never commit it."
            ),
            Finding::NotIgnored { shown } => format!(
                "File {shown} is not protected by .gitignore. Add `config/*` to .gitignore so it is never committed by accident."
            ),
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub enum Language {
    German,
    English,
}

/// Checks the folder without changing anything (cached for a few seconds).
pub fn status(dir: &Path) -> ConfigFilesStatus {
    static CACHE: Mutex<Option<(PathBuf, Instant, Vec<Msg>)>> = Mutex::new(None);
    let warning_msgs = {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        match cache.as_ref() {
            Some((p, at, w)) if p == dir && at.elapsed() < CACHE_TTL => w.clone(),
            _ => {
                let w: Vec<Msg> = findings(dir).iter().map(Finding::msg).collect();
                *cache = Some((dir.to_path_buf(), Instant::now(), w.clone()));
                w
            }
        }
    };
    ConfigFilesStatus {
        path: dir.display().to_string(),
        warnings: warning_msgs.iter().map(Msg::de).collect(),
        warning_msgs,
    }
}

/// Files of the folder that must stay private: every regular file except the example.
fn private_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n != EXAMPLE_FILE)
        .collect();
    names.sort();
    names
}

/// All warnings; changes nothing.
#[cfg(test)]
pub fn check(dir: &Path, language: Language) -> Vec<String> {
    findings(dir)
        .iter()
        .map(|f| match language {
            Language::English => f.english(),
            Language::German => f.msg().de(),
        })
        .collect()
}

fn findings(dir: &Path) -> Vec<Finding> {
    let mut found = Vec::new();
    if let Some(mode) = loose_mode(dir, 0o077) {
        found.push(Finding::Loose {
            path: dir.display().to_string(),
            mode,
        });
    }
    for name in private_files(dir) {
        if let Some(mode) = loose_mode(&dir.join(&name), 0o077) {
            found.push(Finding::Loose {
                path: dir.join(&name).display().to_string(),
                mode,
            });
        }
    }
    found.extend(git_findings(dir));
    found
}

/// At startup: tightens loose permissions (`700` folder, `600` files) and returns what was
/// done plus the remaining warnings.
pub fn check_and_fix(dir: &Path) -> Vec<String> {
    let mut notes = Vec::new();
    let mut fix = |p: &Path, mode: u32| {
        if let Some(old) = loose_mode(p, 0o077) {
            let ok = set_mode(p, mode);
            notes.push(format!(
                "{} was accessible by other users (mode {old:o}){}",
                p.display(),
                if ok {
                    format!(" and was set to {mode:o}")
                } else {
                    format!(" and could not be set to {mode:o}")
                }
            ));
        }
    };
    fix(dir, 0o700);
    for name in private_files(dir) {
        fix(&dir.join(name), 0o600);
    }
    notes.extend(git_findings(dir).iter().map(Finding::english));
    notes
}

#[cfg(unix)]
fn loose_mode(path: &Path, mask: u32) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode() & 0o777;
    (mode & mask != 0).then_some(mode)
}

#[cfg(not(unix))]
fn loose_mode(_path: &Path, _mask: u32) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).is_ok()
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> bool {
    true
}

/// Runs `git -C dir args`; `None` if git is missing, else (success, exit code, stdout).
fn git(dir: &Path, args: &[&str]) -> Option<(bool, i32, String)> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some((
        out.status.success(),
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

fn git_findings(dir: &Path) -> Vec<Finding> {
    if !dir.is_dir() {
        return Vec::new();
    }
    // Silently skip without git or outside a repository.
    match git(dir, &["rev-parse", "--is-inside-work-tree"]) {
        Some((true, _, out)) if out == "true" => {}
        _ => return Vec::new(),
    }
    let mut warnings = Vec::new();
    // Tracked (also files that were deleted but are still in the index).
    let tracked: Vec<String> = git(dir, &["ls-files", "-z", "--", "."])
        .map(|(_, _, out)| out)
        .unwrap_or_default()
        .split('\0')
        .filter(|n| !n.is_empty() && *n != EXAMPLE_FILE)
        .map(String::from)
        .collect();
    for name in &tracked {
        let shown = dir.join(name).display().to_string();
        warnings.push(Finding::Tracked {
            shown,
            name: name.clone(),
        });
    }
    // Exit code 1: not ignored (0: ignored, 128: error – then say nothing).
    for name in private_files(dir) {
        if tracked.contains(&name) {
            continue;
        }
        if let Some((false, 1, _)) = git(dir, &["check-ignore", "-q", "--", &name]) {
            let shown = dir.join(&name).display().to_string();
            warnings.push(Finding::NotIgnored { shown });
        }
    }
    warnings
}

#[cfg(test)]
mod tests;
