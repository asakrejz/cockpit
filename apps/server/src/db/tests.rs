use super::*;

#[test]
fn workspace_is_replaces_not_multiplied() {
    let conn = open_in_memory().unwrap();
    assert_eq!(load_workspace(&conn).unwrap(), None);
    workspace_save(&conn, 1, "eins").unwrap();
    workspace_save(&conn, 2, "zwei").unwrap();
    assert_eq!(load_workspace(&conn).unwrap().as_deref(), Some("zwei"));
    let n: u32 = conn
        .query_row("SELECT COUNT(*) FROM workspace", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn an_unreadable_workspace_is_moved_aside_unchanged() {
    let conn = open_in_memory().unwrap();
    // Nothing stored, nothing to keep.
    assert!(!workspace_keep_unreadable(&conn, "x").unwrap());
    assert_eq!(workspaces_kept(&conn).unwrap(), None);

    workspace_save(&conn, 9, "{kaputt").unwrap();
    assert!(workspace_keep_unreadable(&conn, "not valid JSON").unwrap());
    // Moved, not copied: the row is no longer the stored workspace.
    assert_eq!(load_workspace(&conn).unwrap(), None);
    let (revision, json, error, stored_at, kept_at): (i64, String, String, String, String) = conn
        .query_row(
            "SELECT revision, json, error, stored_at, kept_at FROM workspace_unreadable",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!((revision, json.as_str()), (9, "{kaputt"));
    assert_eq!(error, "not valid JSON");
    assert_eq!(stored_at.len(), 19, "{stored_at}");
    assert_eq!(kept_at.len(), 19, "{kept_at}");

    // Every further one is added; none replaces another.
    workspace_save(&conn, 1, "neu").unwrap();
    assert!(workspace_keep_unreadable(&conn, "again").unwrap());
    let kept = workspaces_kept(&conn).unwrap().unwrap();
    assert_eq!(kept.count, 2);
    assert_eq!(kept.last_kept_at.len(), 19);
}

#[test]
fn the_migration_that_adds_the_kept_workspaces_leaves_a_stored_workspace_alone() {
    // A database as it was before the table existed: the first two migrations applied.
    let mut conn = Connection::open_in_memory().unwrap();
    version(&conn).unwrap();
    for (i, sql) in MIGRATIONS.iter().take(2).enumerate() {
        conn.execute_batch(sql).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![i as u32 + 1],
        )
        .unwrap();
    }
    workspace_save(&conn, 4, "{\"version\":1}").unwrap();

    assert_eq!(migrate(&mut conn).unwrap(), CURRENT_VERSION);
    assert_eq!(
        load_workspace(&conn).unwrap().as_deref(),
        Some("{\"version\":1}")
    );
    assert_eq!(workspaces_kept(&conn).unwrap(), None);
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
    // Data is kept, the version table has exactly one entry per version.
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
