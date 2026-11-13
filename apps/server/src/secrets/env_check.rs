//! Safety net around the `.env` file. The risk of keeping credentials in a file in the repo is
//! accepted, so it is kept small: the file must not be tracked by Git, must be covered by
//! `.gitignore` and must not be readable by others. Warnings are plain German text for the user.
//!
//! Git is asked through the `git` program; without it (or outside a repository) the checks are
//! skipped silently. Nothing here reads the file's content.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long `/api/health` reuses a result (each check starts `git` up to three times).
const CACHE_TTL: Duration = Duration::from_secs(15);

/// What `/api/health` tells about the file: where it is, and what is wrong with it.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct EnvFileStatus {
    pub path: String,
    pub warnings: Vec<String>,
}

/// Checks the file without changing anything (cached for a few seconds).
pub fn status(path: &Path) -> EnvFileStatus {
    static CACHE: Mutex<Option<(std::path::PathBuf, Instant, Vec<String>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, at, w)) = cache.as_ref()
        && p == path
        && at.elapsed() < CACHE_TTL
    {
        return EnvFileStatus {
            path: path.display().to_string(),
            warnings: w.clone(),
        };
    }
    let warnings = check(path);
    *cache = Some((path.to_path_buf(), Instant::now(), warnings.clone()));
    EnvFileStatus {
        path: path.display().to_string(),
        warnings,
    }
}

/// All warnings for the file at `path`; changes nothing.
pub fn check(path: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(mode) = loose_mode(path) {
        warnings.push(format!(
            "Die Datei {} ist für andere Benutzer lesbar (Rechte {mode:o}). Sie sollte nur dir gehören (600); beim nächsten Start des Servers wird das korrigiert.",
            path.display()
        ));
    }
    warnings.extend(git_warnings(path));
    warnings
}

/// At startup: tightens loose permissions to `600` and returns the warnings that remain plus
/// a note for what was corrected.
pub fn check_and_fix(path: &Path) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(mode) = loose_mode(path) {
        match fix_mode(path) {
            Ok(()) => notes.push(format!(
                "Die Datei {} war für andere Benutzer lesbar (Rechte {mode:o}) und wurde auf 600 gesetzt.",
                path.display()
            )),
            Err(_) => notes.push(format!(
                "Die Datei {} ist für andere Benutzer lesbar (Rechte {mode:o}) und ließ sich nicht auf 600 setzen.",
                path.display()
            )),
        }
    }
    notes.extend(git_warnings(path));
    notes
}

/// The permission bits if group or others have any access, `None` if fine (or not applicable).
#[cfg(unix)]
fn loose_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode() & 0o777;
    (mode & 0o077 != 0).then_some(mode)
}

#[cfg(not(unix))]
fn loose_mode(_path: &Path) -> Option<u32> {
    None
}

#[cfg(unix)]
fn fix_mode(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn fix_mode(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Runs `git -C dir args`; `None` if git is missing, `Some(exit success?)` otherwise.
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

fn git_warnings(path: &Path) -> Vec<String> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // Silently skip without git or outside a repository.
    match git(dir, &["rev-parse", "--is-inside-work-tree"]) {
        Some((true, _, out)) if out == "true" => {}
        _ => return Vec::new(),
    }
    let shown = path.display();
    // Tracked: a commit would publish the keys.
    if let Some((true, _, _)) = git(dir, &["ls-files", "--error-unmatch", "--", name]) {
        return vec![format!(
            "Die Datei {shown} wird von Git verfolgt. Ein Commit würde deine Zugangsdaten veröffentlichen. Entferne sie mit „git rm --cached {name}“, committe sie nie und tausche die darin liegenden Schlüssel aus, falls sie schon einmal committet wurden."
        )];
    }
    // Exit code 1: not ignored (0: ignored, 128: error – then say nothing).
    if let Some((false, 1, _)) = git(dir, &["check-ignore", "-q", "--", name]) {
        return vec![format!(
            "Die Datei {shown} ist nicht durch .gitignore geschützt. Trage „{name}“ in die .gitignore ein, damit sie nie versehentlich committet wird."
        )];
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
