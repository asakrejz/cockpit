//! SQLite im Nutzerverzeichnis, mit einfacher Migration über eine Versionstabelle.

use std::path::Path;

use rusqlite::{Connection, params};

/// Migrationen in der Reihenfolge ihrer Version (1, 2, …). **Nie ändern, nur anhängen.**
const MIGRATIONS: &[&str] = &[
    // 1: Einstellungen und Verbindungen – ohne Schlüssel.
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
    // 2: Der Workspace des Server-Modus als eine Zeile (JSON im Format von `cockpit_core::persist`).
    "CREATE TABLE workspace (
         id         INTEGER PRIMARY KEY CHECK (id = 1),
         revision   INTEGER NOT NULL,
         json       TEXT NOT NULL,
         updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
     );",
];

/// Die Version, auf die [`migrate`] die Datenbank bringt.
pub const CURRENT_VERSION: u32 = MIGRATIONS.len() as u32;

#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// Die Datei stammt von einer neueren Version des Programms.
    TooNew {
        file: u32,
        program: u32,
    },
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Sqlite(e) => write!(f, "Datenbankfehler: {e}"),
            DbError::Io(e) => write!(f, "Datenbankordner nicht anlegbar: {e}"),
            DbError::TooNew { file, program } => write!(
                f,
                "Die Datenbank hat Version {file}, dieses Programm kennt nur Version {program}. Bitte aktualisieren"
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

/// Öffnet (und legt bei Bedarf an) die Datenbank und migriert sie.
pub fn open(path: &Path) -> Result<Connection, DbError> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(DbError::Io)?;
    }
    let mut conn = Connection::open(path)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Flüchtige Datenbank für Tests.
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

/// Wendet fehlende Migrationen an, jede in einer eigenen Transaktion. Mehrfach aufrufbar.
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

/// Legt eine Verbindung an oder aktualisiert sie (ohne Schlüssel).
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

/// Der gespeicherte Workspace des Server-Modus (JSON mit Revision), falls es einen gibt.
pub fn workspace_laden(conn: &Connection) -> Result<Option<String>, DbError> {
    match conn.query_row("SELECT json FROM workspace WHERE id = 1", [], |r| r.get(0)) {
        Ok(json) => Ok(Some(json)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Ersetzt den gespeicherten Workspace.
pub fn workspace_save(conn: &Connection, revision: u64, json: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO workspace (id, revision, json) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET revision = ?1, json = ?2, updated_at = CURRENT_TIMESTAMP",
        params![revision as i64, json],
    )?;
    Ok(())
}

/// Namen aller gespeicherten Verbindungen.
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
mod tests {
    use super::*;

    #[test]
    fn workspace_is_replaces_not_multiplied() {
        let conn = open_in_memory().unwrap();
        assert_eq!(workspace_laden(&conn).unwrap(), None);
        workspace_save(&conn, 1, "eins").unwrap();
        workspace_save(&conn, 2, "zwei").unwrap();
        assert_eq!(workspace_laden(&conn).unwrap().as_deref(), Some("zwei"));
        let n: u32 = conn
            .query_row("SELECT COUNT(*) FROM workspace", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn fresh_database_is_migrated() {
        let conn = open_in_memory().unwrap();
        assert_eq!(version(&conn).unwrap(), CURRENT_VERSION);
        // Tabellen existieren.
        assert_eq!(connections(&conn).unwrap(), Vec::<String>::new());
        assert_eq!(setting(&conn, "x").unwrap(), None);
    }

    #[test]
    fn migration_is_repeatable() {
        let mut conn = open_in_memory().unwrap();
        setting_set(&conn, "thema", "dunkel").unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), CURRENT_VERSION);
        assert_eq!(migrate(&mut conn).unwrap(), CURRENT_VERSION);
        // Daten bleiben erhalten, die Versionstabelle hat genau einen Eintrag je Version.
        assert_eq!(setting(&conn, "thema").unwrap().as_deref(), Some("dunkel"));
        let n: u32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, CURRENT_VERSION);
    }

    #[test]
    fn newer_database_is_not_touched() {
        let mut conn = Connection::open_in_memory().unwrap();
        version(&conn).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![CURRENT_VERSION + 1],
        )
        .unwrap();
        assert!(matches!(migrate(&mut conn), Err(DbError::TooNew { .. })));
    }

    #[test]
    fn connections_and_settings() {
        let conn = open_in_memory().unwrap();
        connection_set(&conn, "jira", "https://a.atlassian.net", Some("a@b.de")).unwrap();
        connection_set(&conn, "github", "https://api.github.com", None).unwrap();
        connection_set(&conn, "jira", "https://b.atlassian.net", Some("c@d.de")).unwrap();
        assert_eq!(connections(&conn).unwrap(), vec!["github", "jira"]);
        let url: String = conn
            .query_row(
                "SELECT base_url FROM connections WHERE name = 'jira'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(url, "https://b.atlassian.net");
        setting_set(&conn, "k", "1").unwrap();
        setting_set(&conn, "k", "2").unwrap();
        assert_eq!(setting(&conn, "k").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn file_is_with_dir_created() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("unter/ordner/cockpit.db");
        {
            let conn = open(&path).unwrap();
            setting_set(&conn, "a", "b").unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(setting(&conn, "a").unwrap().as_deref(), Some("b"));
    }
}
