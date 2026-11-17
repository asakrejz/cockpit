//! Safety net around the folder `config/`: its files must not be tracked by Git (only
//! `config.example.toml` may be), must be covered by `.gitignore` and must not be readable by
//! others. Counterpart of `secrets/env_check`; nothing here reads a file's content.
//!
//! Without `git` or outside a repository the Git checks are skipped silently.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::location::EXAMPLE_FILE;

/// How long `/api/health` reuses a result (the check starts `git` a few times).
const CACHE_TTL: Duration = Duration::from_secs(15);

/// What `/api/health` tells about the folder: where it is, what is wrong, and old files.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ConfigFilesStatus {
    pub path: String,
    pub warnings: Vec<String>,
    /// Hints about old files that can be deleted (German, for the interface).
    pub notices: Vec<String>,
}

#[derive(Clone, Copy)]
pub enum Language {
    German,
    English,
}

/// Checks the folder without changing anything (cached for a few seconds); `legacy` are the
/// places of the old files, reported while they still exist.
pub fn status(dir: &Path, legacy: &[PathBuf]) -> ConfigFilesStatus {
    static CACHE: Mutex<Option<(PathBuf, Instant, Vec<String>)>> = Mutex::new(None);
    let warnings = {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        match cache.as_ref() {
            Some((p, at, w)) if p == dir && at.elapsed() < CACHE_TTL => w.clone(),
            _ => {
                let w = check(dir, Language::German);
                *cache = Some((dir.to_path_buf(), Instant::now(), w.clone()));
                w
            }
        }
    };
    ConfigFilesStatus {
        path: dir.display().to_string(),
        warnings,
        notices: legacy_notices(legacy),
    }
}

/// "Old configuration is still at …": one line per old folder that still has files.
pub fn legacy_notices(legacy: &[PathBuf]) -> Vec<String> {
    let mut folders: Vec<&Path> = Vec::new();
    for f in legacy.iter().filter(|f| f.is_file()) {
        if let Some(d) = f.parent()
            && !folders.contains(&d)
        {
            folders.push(d);
        }
    }
    folders
        .into_iter()
        .map(|d| {
            format!(
                "Alte Konfiguration liegt noch unter {}, du kannst sie löschen. Cockpit nutzt nur noch den Ordner config im Projekt.",
                d.display()
            )
        })
        .collect()
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
pub fn check(dir: &Path, language: Language) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(mode) = loose_mode(dir, 0o077) {
        warnings.push(loose(dir, mode, language));
    }
    for name in private_files(dir) {
        if let Some(mode) = loose_mode(&dir.join(&name), 0o077) {
            warnings.push(loose(&dir.join(&name), mode, language));
        }
    }
    warnings.extend(git_warnings(dir, language));
    warnings
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
    notes.extend(git_warnings(dir, Language::English));
    notes
}

fn loose(path: &Path, mode: u32, language: Language) -> String {
    match language {
        Language::English => format!(
            "{} is accessible by other users (mode {mode:o}); it should belong to you alone.",
            path.display()
        ),
        Language::German => format!(
            "{} ist für andere Benutzer zugänglich (Rechte {mode:o}). Sie sollte nur dir gehören (Ordner 700, Dateien 600); beim nächsten Start des Servers wird das korrigiert.",
            path.display()
        ),
    }
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

fn git_warnings(dir: &Path, language: Language) -> Vec<String> {
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
        warnings.push(match language {
            Language::English => format!(
                "File {shown} is tracked by Git. A commit would publish your configuration. Remove it with `git rm --cached config/{name}` and never commit it."
            ),
            Language::German => format!(
                "Die Datei {shown} wird von Git verfolgt. Ein Commit würde deine Konfiguration veröffentlichen. Entferne sie mit „git rm --cached config/{name}“ und committe sie nie."
            ),
        });
    }
    // Exit code 1: not ignored (0: ignored, 128: error – then say nothing).
    for name in private_files(dir) {
        if tracked.contains(&name) {
            continue;
        }
        if let Some((false, 1, _)) = git(dir, &["check-ignore", "-q", "--", &name]) {
            let shown = dir.join(&name).display().to_string();
            warnings.push(match language {
                Language::English => format!(
                    "File {shown} is not protected by .gitignore. Add `config/*` to .gitignore so it is never committed by accident."
                ),
                Language::German => format!(
                    "Die Datei {shown} ist nicht durch .gitignore geschützt. Trage „config/*“ in die .gitignore ein, damit sie nie versehentlich committet wird."
                ),
            });
        }
    }
    warnings
}

#[cfg(test)]
mod tests;
