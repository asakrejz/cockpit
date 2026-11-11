//! SQLite im Nutzerverzeichnis, mit einfacher Migration über eine Versionstabelle.

use std::path::Path;

use rusqlite::{Connection, params};

/// Migrationen in der Reihenfolge ihrer Version (1, 2, …). **Nie ändern, nur anhängen.**
const MIGRATIONEN: &[&str] = &[
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
pub const AKTUELLE_VERSION: u32 = MIGRATIONEN.len() as u32;

#[derive(Debug)]
pub enum DbFehler {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// Die Datei stammt von einer neueren Version des Programms.
    ZuNeu {
        datei: u32,
        programm: u32,
    },
}

impl std::fmt::Display for DbFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbFehler::Sqlite(e) => write!(f, "Datenbankfehler: {e}"),
            DbFehler::Io(e) => write!(f, "Datenbankordner nicht anlegbar: {e}"),
            DbFehler::ZuNeu { datei, programm } => write!(
                f,
                "Die Datenbank hat Version {datei}, dieses Programm kennt nur Version {programm}. Bitte aktualisieren"
            ),
        }
    }
}

impl std::error::Error for DbFehler {}

impl From<rusqlite::Error> for DbFehler {
    fn from(e: rusqlite::Error) -> Self {
        DbFehler::Sqlite(e)
    }
}

/// Öffnet (und legt bei Bedarf an) die Datenbank und migriert sie.
pub fn open(pfad: &Path) -> Result<Connection, DbFehler> {
    if let Some(ordner) = pfad.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(ordner).map_err(DbFehler::Io)?;
    }
    let mut conn = Connection::open(pfad)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Flüchtige Datenbank für Tests.
#[cfg(test)]
pub fn open_im_speicher() -> Result<Connection, DbFehler> {
    let mut conn = Connection::open_in_memory()?;
    migrate(&mut conn)?;
    Ok(conn)
}

pub fn version(conn: &Connection) -> Result<u32, DbFehler> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)")?;
    let v: Option<u32> =
        conn.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))?;
    Ok(v.unwrap_or(0))
}

/// Wendet fehlende Migrationen an, jede in einer eigenen Transaktion. Mehrfach aufrufbar.
pub fn migrate(conn: &mut Connection) -> Result<u32, DbFehler> {
    let aktuell = version(conn)?;
    if aktuell > AKTUELLE_VERSION {
        return Err(DbFehler::ZuNeu {
            datei: aktuell,
            programm: AKTUELLE_VERSION,
        });
    }
    for (i, sql) in MIGRATIONEN.iter().enumerate().skip(aktuell as usize) {
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
pub fn verbindung_setzen(
    conn: &Connection,
    name: &str,
    base_url: &str,
    account: Option<&str>,
) -> Result<(), DbFehler> {
    conn.execute(
        "INSERT INTO connections (name, base_url, account) VALUES (?1, ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET base_url = ?2, account = ?3, updated_at = CURRENT_TIMESTAMP",
        params![name, base_url, account],
    )?;
    Ok(())
}

/// Der gespeicherte Workspace des Server-Modus (JSON mit Revision), falls es einen gibt.
pub fn workspace_laden(conn: &Connection) -> Result<Option<String>, DbFehler> {
    match conn.query_row("SELECT json FROM workspace WHERE id = 1", [], |r| r.get(0)) {
        Ok(json) => Ok(Some(json)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Ersetzt den gespeicherten Workspace.
pub fn workspace_speichern(conn: &Connection, revision: u64, json: &str) -> Result<(), DbFehler> {
    conn.execute(
        "INSERT INTO workspace (id, revision, json) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET revision = ?1, json = ?2, updated_at = CURRENT_TIMESTAMP",
        params![revision as i64, json],
    )?;
    Ok(())
}

/// Namen aller gespeicherten Verbindungen.
pub fn verbindungen(conn: &Connection) -> Result<Vec<String>, DbFehler> {
    let mut stmt = conn.prepare("SELECT name FROM connections ORDER BY name")?;
    let namen = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(namen)
}

// Noch ohne Aufrufer im Server; die Einstellungen kommen mit den nächsten Funktionen.
#[allow(dead_code)]
pub fn einstellung_setzen(conn: &Connection, key: &str, value: &str) -> Result<(), DbFehler> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, value],
    )?;
    Ok(())
}

#[allow(dead_code)]
pub fn einstellung(conn: &Connection, key: &str) -> Result<Option<String>, DbFehler> {
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
    fn workspace_wird_ersetzt_nicht_vermehrt() {
        let conn = open_im_speicher().unwrap();
        assert_eq!(workspace_laden(&conn).unwrap(), None);
        workspace_speichern(&conn, 1, "eins").unwrap();
        workspace_speichern(&conn, 2, "zwei").unwrap();
        assert_eq!(workspace_laden(&conn).unwrap().as_deref(), Some("zwei"));
        let n: u32 = conn
            .query_row("SELECT COUNT(*) FROM workspace", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn frische_datenbank_wird_migriert() {
        let conn = open_im_speicher().unwrap();
        assert_eq!(version(&conn).unwrap(), AKTUELLE_VERSION);
        // Tabellen existieren.
        assert_eq!(verbindungen(&conn).unwrap(), Vec::<String>::new());
        assert_eq!(einstellung(&conn, "x").unwrap(), None);
    }

    #[test]
    fn migration_ist_wiederholbar() {
        let mut conn = open_im_speicher().unwrap();
        einstellung_setzen(&conn, "thema", "dunkel").unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), AKTUELLE_VERSION);
        assert_eq!(migrate(&mut conn).unwrap(), AKTUELLE_VERSION);
        // Daten bleiben erhalten, die Versionstabelle hat genau einen Eintrag je Version.
        assert_eq!(
            einstellung(&conn, "thema").unwrap().as_deref(),
            Some("dunkel")
        );
        let n: u32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, AKTUELLE_VERSION);
    }

    #[test]
    fn neuere_datenbank_wird_nicht_angefasst() {
        let mut conn = Connection::open_in_memory().unwrap();
        version(&conn).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![AKTUELLE_VERSION + 1],
        )
        .unwrap();
        assert!(matches!(migrate(&mut conn), Err(DbFehler::ZuNeu { .. })));
    }

    #[test]
    fn verbindungen_und_einstellungen() {
        let conn = open_im_speicher().unwrap();
        verbindung_setzen(&conn, "jira", "https://a.atlassian.net", Some("a@b.de")).unwrap();
        verbindung_setzen(&conn, "github", "https://api.github.com", None).unwrap();
        verbindung_setzen(&conn, "jira", "https://b.atlassian.net", Some("c@d.de")).unwrap();
        assert_eq!(verbindungen(&conn).unwrap(), vec!["github", "jira"]);
        let url: String = conn
            .query_row(
                "SELECT base_url FROM connections WHERE name = 'jira'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(url, "https://b.atlassian.net");
        einstellung_setzen(&conn, "k", "1").unwrap();
        einstellung_setzen(&conn, "k", "2").unwrap();
        assert_eq!(einstellung(&conn, "k").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn datei_wird_samt_ordner_angelegt() {
        let tmp = tempfile::tempdir().unwrap();
        let pfad = tmp.path().join("unter/ordner/cockpit.db");
        {
            let conn = open(&pfad).unwrap();
            einstellung_setzen(&conn, "a", "b").unwrap();
        }
        let conn = open(&pfad).unwrap();
        assert_eq!(einstellung(&conn, "a").unwrap().as_deref(), Some("b"));
    }
}
