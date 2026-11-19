//! Where configuration and database live: the folder `config/` in the project folder.
//!
//! * `<project>/config/config.toml` and `<project>/config/cockpit.db` are the defaults; the folder
//!   is created at the first start (mode `700`, files `600`), an empty `config.toml` too.
//! * `<project>/config/.env` holds the secrets (`COCKPIT_ENV_FILE` overrides); an old
//!   `<project>/.env` is moved there at the start if `config/.env` is missing.
//! * "Project folder" is the working directory (`scripts/start.sh`, `start-cockpit.command` and the launchd template all start in it).
//! * `COCKPIT_CONFIG` and `COCKPIT_DB` win; they switch off creating and migrating.
//! * One-time migration: if the new file is missing but the old one exists (macOS
//!   `~/Library/Application Support/cockpit/`, Linux `$XDG_CONFIG_HOME` or `~/.config/cockpit/`,
//!   database in the data folder), it is **copied**. Nothing is deleted or touched at the old place.

use std::io::Write;
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

/// The place of the secrets file before it moved into `config/`: `<project>/.env`.
pub fn old_env_path(project: &Path) -> PathBuf {
    project.join(crate::secrets::ENV_FILE_NAME)
}

/// Moves the secrets file from `<project>/.env` into `config/` (rename, so mode `600` stays).
/// Only for the default place (`COCKPIT_ENV_FILE` unset) and only if `config/.env` is missing.
/// Returns the message for the log.
pub fn migrate_env(project: &Path, env: Env) -> Result<Option<String>, String> {
    if set(env, crate::secrets::ENV_FILE_VAR) {
        return Ok(None);
    }
    let (old, new) = (old_env_path(project), default_env_path(project));
    if !old.is_file() || new.exists() {
        return Ok(None);
    }
    ensure_dir(&config_dir(project))?;
    std::fs::rename(&old, &new).map_err(|e| {
        format!(
            "Cannot move {} to {} ({})",
            old.display(),
            new.display(),
            e.kind()
        )
    })?;
    Ok(Some(format!(
        "Moved the secrets file from {} to {}",
        old.display(),
        new.display()
    )))
}

/// The old `<project>/.env`, while it still exists next to a `config/.env` that is in use
/// (shown in `/api/health`; nothing is touched). Not for an overridden `COCKPIT_ENV_FILE`.
pub fn legacy_env_candidate(project: &Path, env: Env) -> Option<PathBuf> {
    if set(env, crate::secrets::ENV_FILE_VAR) {
        return None;
    }
    let old = old_env_path(project);
    (old.is_file() && default_env_path(project).exists()).then_some(old)
}

/// The places where older versions kept their files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legacy {
    pub config: PathBuf,
    pub db: PathBuf,
}

/// `macos`: use the layout of macOS (`Application Support`), otherwise XDG.
pub fn legacy_locations(env: Env, macos: bool) -> Option<Legacy> {
    let home = env("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)?;
    let xdg = |name: &str, fallback: &str| {
        env(name)
            .filter(|v| !v.is_empty() && Path::new(v).is_absolute())
            .map_or_else(|| home.join(fallback), PathBuf::from)
    };
    let (config_base, data_base) = if macos {
        let base = home.join("Library").join("Application Support");
        (base.clone(), base)
    } else {
        (
            xdg("XDG_CONFIG_HOME", ".config"),
            xdg("XDG_DATA_HOME", ".local/share"),
        )
    };
    Some(Legacy {
        config: config_base.join("cockpit").join(CONFIG_FILE),
        db: data_base.join("cockpit").join(DB_FILE),
    })
}

/// Whether this build uses the macOS layout of the old files.
pub fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

/// Old files that still exist, only while no override is set (shown in `/api/health`).
pub fn legacy_candidates(env: Env, macos: bool) -> Vec<PathBuf> {
    if set(env, "COCKPIT_CONFIG") || set(env, "COCKPIT_DB") {
        return Vec::new();
    }
    legacy_locations(env, macos)
        .map(|l| vec![l.config, l.db])
        .unwrap_or_default()
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

/// Copies `from` to `to` without ever overwriting (`create_new`), mode `600`.
fn copy_new(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut src = std::fs::File::open(from)?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut dst = opts.open(to)?;
    std::io::copy(&mut src, &mut dst)?;
    dst.flush()?;
    chmod(to, 0o600);
    Ok(())
}

/// Copies a SQLite database consistently: `VACUUM INTO` from a read-only connection (includes
/// the content of a `-wal` file and leaves the old files as they are). If that is not possible,
/// the files are copied (`-wal` too, so no committed data is lost).
fn copy_db(from: &Path, to: &Path) -> Result<(), String> {
    let tmp = to.with_extension("db.migrating");
    let _ = std::fs::remove_file(&tmp);
    let vacuum = (|| -> rusqlite::Result<()> {
        let conn = rusqlite::Connection::open_with_flags(
            from,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.execute("VACUUM INTO ?1", [tmp.to_string_lossy().as_ref()])?;
        Ok(())
    })();
    if vacuum.is_err() {
        let _ = std::fs::remove_file(&tmp);
        copy_new(from, &tmp).map_err(|e| format!("copy failed ({})", e.kind()))?;
        let wal = PathBuf::from(format!("{}-wal", from.display()));
        if wal.is_file() {
            let wal_to = PathBuf::from(format!("{}-wal", tmp.display()));
            let _ = std::fs::remove_file(&wal_to);
            copy_new(&wal, &wal_to).map_err(|e| format!("copy failed ({})", e.kind()))?;
        }
    }
    chmod(&tmp, 0o600);
    // Never replace a file that appeared meanwhile.
    if to.exists() {
        let _ = std::fs::remove_file(&tmp);
        return Ok(());
    }
    // If the fallback copy left a `-wal`, SQLite replays it when the renamed file is opened:
    // it must carry the final name.
    let wal_tmp = PathBuf::from(format!("{}-wal", tmp.display()));
    let wal_to = PathBuf::from(format!("{}-wal", to.display()));
    if wal_tmp.is_file() {
        std::fs::rename(&wal_tmp, &wal_to).map_err(|e| format!("rename failed ({})", e.kind()))?;
    }
    std::fs::rename(&tmp, to).map_err(|e| format!("rename failed ({})", e.kind()))
}

/// Prepares the default locations: creates `config/` and an empty `config.toml`, and copies old
/// files once. Returns the messages for the log. Does nothing if both overrides are set.
pub fn prepare(project: &Path, env: Env, macos: bool) -> Result<Vec<String>, String> {
    let config_explicit = set(env, "COCKPIT_CONFIG");
    let db_explicit = set(env, "COCKPIT_DB");
    let mut notes = Vec::new();
    if config_explicit && db_explicit {
        return Ok(notes);
    }
    let dir = config_dir(project);
    ensure_dir(&dir)?;
    let migrate = !config_explicit && !db_explicit;
    let legacy = if migrate {
        legacy_locations(env, macos)
    } else {
        None
    };

    if !config_explicit {
        let target = default_config_path(project);
        if !target.exists() {
            match legacy.as_ref().filter(|l| l.config.is_file()) {
                Some(l) => {
                    copy_new(&l.config, &target).map_err(|e| {
                        format!("Cannot copy {} ({})", l.config.display(), e.kind())
                    })?;
                    notes.push(format!(
                        "Copied config from {} to {}; the old file was left in place and can be deleted",
                        l.config.display(),
                        target.display()
                    ));
                }
                None => {
                    let _ = create_empty(&target);
                }
            }
        }
    }
    if let Some(l) = legacy.as_ref().filter(|l| l.db.is_file()) {
        let target = default_db_path(project);
        if !target.exists() {
            copy_db(&l.db, &target)
                .map_err(|e| format!("Cannot copy database {} ({e})", l.db.display()))?;
            notes.push(format!(
                "Copied database from {} to {}; the old files were left in place and can be deleted",
                l.db.display(),
                target.display()
            ));
        }
    }
    Ok(notes)
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
