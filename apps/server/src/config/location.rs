//! Where configuration and database live: the folder `config/` in the project folder.
//!
//! * `<project>/config/config.toml` and `<project>/config/cockpit.db` are the defaults; the folder
//!   is created at the first start (mode `700`, files `600`), an empty `config.toml` too.
//! * `<project>/config/.env` holds the secrets (`COCKPIT_ENV_FILE` overrides).
//! * "Project folder" is the working directory (`scripts/start.sh`, `start-cockpit.command` and the launchd template all start in it).
//! * `COCKPIT_CONFIG` and `COCKPIT_DB` win; they switch off creating.

use std::path::{Path, PathBuf};

/// Name of the folder inside the project folder.
pub const DIR_NAME: &str = "config";
pub const CONFIG_FILE: &str = "config.toml";
pub const DB_FILE: &str = "cockpit.db";
/// The only file in `config/` that Git may track.
pub const EXAMPLE_FILE: &str = "config.example.toml";

/// Environment lookup (replaceable in tests).
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn set(env: Env, name: &str) -> bool {
    env(name).is_some_and(|v| !v.is_empty())
}

/// The project folder: the working directory, the same as for the `.env` file.
pub fn project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

pub fn config_dir(project: &Path) -> PathBuf {
    project.join(DIR_NAME)
}

pub fn default_config_path(project: &Path) -> PathBuf {
    config_dir(project).join(CONFIG_FILE)
}

pub fn default_db_path(project: &Path) -> PathBuf {
    config_dir(project).join(DB_FILE)
}

/// The default secrets file: `<project>/config/.env`.
pub fn default_env_path(project: &Path) -> PathBuf {
    config_dir(project).join(crate::secrets::ENV_FILE_NAME)
}

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn chmod(_path: &Path, _mode: u32) {}

/// Creates `dir` (mode `700`) if needed.
fn ensure_dir(dir: &Path) -> Result<(), String> {
    let existed = dir.is_dir();
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Cannot create folder {} ({})", dir.display(), e.kind()))?;
    if !existed {
        chmod(dir, 0o700);
    }
    Ok(())
}

/// Prepares the default locations: creates `config/` and an empty `config.toml`. Does nothing if
/// both overrides are set.
pub fn prepare(project: &Path, env: Env) -> Result<(), String> {
    let config_explicit = set(env, "COCKPIT_CONFIG");
    if config_explicit && set(env, "COCKPIT_DB") {
        return Ok(());
    }
    ensure_dir(&config_dir(project))?;
    if !config_explicit {
        let target = default_config_path(project);
        if !target.exists() {
            let _ = create_empty(&target);
        }
    }
    Ok(())
}

/// An empty file, only if there is none (never overwrites).
fn create_empty(target: &Path) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(target).map(|_| ())
}

/// Sets the database file (and a left-over `-wal`/`-shm`) to mode `600`.
pub fn tighten_db(db: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let p = PathBuf::from(format!("{}{suffix}", db.display()));
        if p.is_file() {
            chmod(&p, 0o600);
        }
    }
}

#[cfg(test)]
mod tests;
