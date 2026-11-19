//! SQLite in the user directory, with simple migration via a version table.

use std::path::Path;

use rusqlite::{Connection, params};

/// Migrations in the order of their version (1, 2, …). **Never change, only append.**
const MIGRATIONS: &[&str] = &[
    // 1: settings and connections – without keys.
    "CREATE TABLE settings (
         key   TEXT PRIMARY KEY,
         value TEXT NOT NULL
     );
     CREATE TABLE connections (
         name       TEXT PRIMARY KEY,   -- 'github' | 'jira'
         base_url   TEXT NOT NULL,
         account    TEXT,               -- Jira: E-Mail; GitHub: später der Login
         updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
     );",
    // 2: the workspace of the server mode as one row (JSON in the format of `cockpit_core::persist`).
    "CREATE TABLE workspace (
         id         INTEGER PRIMARY KEY CHECK (id = 1),
         revision   INTEGER NOT NULL,
         json       TEXT NOT NULL,
         updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
     );",
];

/// The version to which [`migrate`] brings the database.
pub const CURRENT_VERSION: u32 = MIGRATIONS.len() as u32;

#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// The file comes from a newer version of the program.
    TooNew {
        file: u32,
        program: u32,
    },
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Sqlite(e) => write!(f, "Database error: {e}"),
            DbError::Io(e) => write!(f, "Cannot create database folder: {e}"),
            DbError::TooNew { file, program } => write!(
                f,
                "The database has version {file}, this program only knows version {program}. Please update"
            ),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

/// Opens (and creates if needed) the database and migrates it.
pub fn open(path: &Path) -> Result<Connection, DbError> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(DbError::Io)?;
    }
    let mut conn = Connection::open(path)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Volatile database for tests.
#[cfg(test)]
pub fn open_in_memory() -> Result<Connection, DbError> {
    let mut conn = Connection::open_in_memory()?;
    migrate(&mut conn)?;
    Ok(conn)
}

pub fn version(conn: &Connection) -> Result<u32, DbError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)")?;
    let v: Option<u32> =
        conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))?;
    Ok(v.unwrap_or(0))
}

/// Applies missing migrations, each in its own transaction. Can be called repeatedly.
pub fn migrate(conn: &mut Connection) -> Result<u32, DbError> {
    let current = version(conn)?;
    if current > CURRENT_VERSION {
        return Err(DbError::TooNew {
            file: current,
            program: CURRENT_VERSION,
        });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![i as u32 + 1],
        )?;
        tx.commit()?;
    }
    version(conn)
}

/// Creates a connection or updates it (without a key).
pub fn connection_set(
    conn: &Connection,
    name: &str,
    base_url: &str,
    account: Option<&str>,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO connections (name, base_url, account) VALUES (?1, ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET base_url = ?2, account = ?3, updated_at = CURRENT_TIMESTAMP",
        params![name, base_url, account],
    )?;
    Ok(())
}

/// The stored workspace of the server mode (JSON with revision), if there is one.
pub fn load_workspace(conn: &Connection) -> Result<Option<String>, DbError> {
    match conn.query_row("SELECT json FROM workspace WHERE id = 1", [], |r| r.get(0)) {
        Ok(json) => Ok(Some(json)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Replaces the stored workspace.
pub fn workspace_save(conn: &Connection, revision: u64, json: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO workspace (id, revision, json) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET revision = ?1, json = ?2, updated_at = CURRENT_TIMESTAMP",
        params![revision as i64, json],
    )?;
    Ok(())
}

/// Names of all stored connections (the status no longer reads them: the key store decides).
#[cfg(test)]
pub fn connections(conn: &Connection) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare("SELECT name FROM connections ORDER BY name")?;
    let names = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(names)
}

pub fn setting_set(conn: &Connection, key: &str, value: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, value],
    )?;
    Ok(())
}

pub fn setting_delete(conn: &Connection, key: &str) -> Result<(), DbError> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

pub fn setting(conn: &Connection, key: &str) -> Result<Option<String>, DbError> {
    match conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |r| r.get(0),
    ) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests;
