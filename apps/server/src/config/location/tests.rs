use super::*;
use std::collections::HashMap;

fn env_of(pairs: &[(&str, String)]) -> impl Fn(&str) -> Option<String> + use<> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect();
    move |k| map.get(k).cloned()
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

/// Rebuilds the old structure below a temporary `HOME` (macOS layout).
fn old_macos(home: &Path) -> PathBuf {
    let d = home.join("Library/Application Support/cockpit");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("config.toml"), "port = 9001\n").unwrap();
    d
}

fn make_db(path: &Path, wal: bool) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(path).unwrap();
    if wal {
        let _: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .unwrap();
        conn.execute("PRAGMA wal_autocheckpoint=0", []).ok();
    }
    conn.execute("CREATE TABLE t (v TEXT)", []).unwrap();
    conn.execute("INSERT INTO t VALUES ('old row')", [])
        .unwrap();
    conn
}

fn rows(path: &Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut st = conn.prepare("SELECT v FROM t").unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn default_paths_are_in_the_config_folder_of_the_project() {
    let project = Path::new("/work/cockpit");
    assert_eq!(config_dir(project), Path::new("/work/cockpit/config"));
    assert_eq!(
        default_config_path(project),
        Path::new("/work/cockpit/config/config.toml")
    );
    assert_eq!(
        default_db_path(project),
        Path::new("/work/cockpit/config/cockpit.db")
    );
}

#[test]
fn project_dir_is_the_working_directory_like_the_env_file() {
    let p = project_dir();
    assert_eq!(p, std::env::current_dir().unwrap());
    let env_default = crate::secrets::EnvFileStore::new(p.join(".env"));
    assert_eq!(env_default.path().parent(), Some(p.as_path()));
}

#[test]
fn legacy_places_follow_home_and_xdg() {
    let env = env_of(&[("HOME", "/h".into())]);
    let mac = legacy_locations(&env, true).unwrap();
    assert_eq!(
        mac.config,
        Path::new("/h/Library/Application Support/cockpit/config.toml")
    );
    assert_eq!(
        mac.db,
        Path::new("/h/Library/Application Support/cockpit/cockpit.db")
    );
    let lin = legacy_locations(&env, false).unwrap();
    assert_eq!(lin.config, Path::new("/h/.config/cockpit/config.toml"));
    assert_eq!(lin.db, Path::new("/h/.local/share/cockpit/cockpit.db"));
    let env = env_of(&[
        ("HOME", "/h".into()),
        ("XDG_CONFIG_HOME", "/x/c".into()),
        ("XDG_DATA_HOME", "/x/d".into()),
    ]);
    let lin = legacy_locations(&env, false).unwrap();
    assert_eq!(lin.config, Path::new("/x/c/cockpit/config.toml"));
    assert_eq!(lin.db, Path::new("/x/d/cockpit/cockpit.db"));
    // A relative XDG value is ignored (as the specification says).
    let env = env_of(&[("HOME", "/h".into()), ("XDG_CONFIG_HOME", "rel".into())]);
    assert_eq!(
        legacy_locations(&env, false).unwrap().config,
        Path::new("/h/.config/cockpit/config.toml")
    );
}

#[test]
fn legacy_candidates_are_empty_with_an_override() {
    let home = tempfile::tempdir().unwrap();
    let h = s(home.path());
    assert_eq!(
        legacy_candidates(&env_of(&[("HOME", h.clone())]), true).len(),
        2
    );
    let with = env_of(&[("HOME", h.clone()), ("COCKPIT_DB", "/x.db".into())]);
    assert!(legacy_candidates(&with, true).is_empty());
    let with = env_of(&[("HOME", h), ("COCKPIT_CONFIG", "/x.toml".into())]);
    assert!(legacy_candidates(&with, true).is_empty());
}

#[cfg(unix)]
#[test]
fn first_start_creates_the_folder_and_an_empty_config_with_tight_modes() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[("HOME", s(home.path()))]);
    let notes = prepare(project.path(), &env, true).unwrap();
    assert!(notes.is_empty());
    let dir = project.path().join("config");
    assert_eq!(mode(&dir), 0o700);
    let file = dir.join("config.toml");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "");
    assert_eq!(mode(&file), 0o600);
    // A second start leaves everything as it is.
    std::fs::write(&file, "port = 1234\n").unwrap();
    assert!(prepare(project.path(), &env, true).unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "port = 1234\n");
}

#[test]
fn both_overrides_create_nothing() {
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[
        ("COCKPIT_CONFIG", "/somewhere/c.toml".into()),
        ("COCKPIT_DB", "/somewhere/c.db".into()),
    ]);
    assert!(prepare(project.path(), &env, true).unwrap().is_empty());
    assert!(!project.path().join("config").exists());
}

#[test]
fn an_explicit_config_path_is_never_created_or_migrated() {
    let home = tempfile::tempdir().unwrap();
    old_macos(home.path());
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[
        ("HOME", s(home.path())),
        ("COCKPIT_CONFIG", s(&project.path().join("mine.toml"))),
    ]);
    assert!(prepare(project.path(), &env, true).unwrap().is_empty());
    assert!(!project.path().join("mine.toml").exists());
    assert!(!project.path().join("config/config.toml").exists());
}

#[test]
fn an_explicit_db_path_stops_the_migration_of_both_files() {
    let home = tempfile::tempdir().unwrap();
    let old = old_macos(home.path());
    make_db(&old.join("cockpit.db"), false);
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[
        ("HOME", s(home.path())),
        ("COCKPIT_DB", s(&project.path().join("x.db"))),
    ]);
    prepare(project.path(), &env, true).unwrap();
    assert_eq!(
        std::fs::read_to_string(project.path().join("config/config.toml")).unwrap(),
        ""
    );
    assert!(!project.path().join("config/cockpit.db").exists());
}

#[cfg(unix)]
#[test]
fn migration_copies_config_and_database_and_deletes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let old = old_macos(home.path());
    drop(make_db(&old.join("cockpit.db"), false));
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[("HOME", s(home.path()))]);

    let notes = prepare(project.path(), &env, true).unwrap();
    assert_eq!(notes.len(), 2);
    assert!(notes[0].starts_with("Copied config from "));
    assert!(notes[0].contains("left in place and can be deleted"));
    assert!(notes[1].starts_with("Copied database from "));

    let new = project.path().join("config");
    assert_eq!(
        std::fs::read_to_string(new.join("config.toml")).unwrap(),
        "port = 9001\n"
    );
    assert_eq!(rows(&new.join("cockpit.db")), ["old row"]);
    assert_eq!(mode(&new.join("config.toml")), 0o600);
    assert_eq!(mode(&new.join("cockpit.db")), 0o600);
    assert!(!new.join("cockpit.db.migrating").exists());
    // The old files stay.
    assert!(old.join("config.toml").is_file());
    assert!(old.join("cockpit.db").is_file());

    // Next start: nothing more is copied, even if the old file changes.
    std::fs::write(old.join("config.toml"), "port = 1\n").unwrap();
    assert!(prepare(project.path(), &env, true).unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(new.join("config.toml")).unwrap(),
        "port = 9001\n"
    );
}

#[test]
fn migration_never_overwrites_an_existing_target() {
    let home = tempfile::tempdir().unwrap();
    let old = old_macos(home.path());
    drop(make_db(&old.join("cockpit.db"), false));
    let project = tempfile::tempdir().unwrap();
    let new = project.path().join("config");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("config.toml"), "port = 7\n").unwrap();
    drop(make_db(&new.join("cockpit.db"), false));
    rusqlite::Connection::open(new.join("cockpit.db"))
        .unwrap()
        .execute("INSERT INTO t VALUES ('new row')", [])
        .unwrap();
    let env = env_of(&[("HOME", s(home.path()))]);
    assert!(prepare(project.path(), &env, true).unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(new.join("config.toml")).unwrap(),
        "port = 7\n"
    );
    assert_eq!(rows(&new.join("cockpit.db")), ["old row", "new row"]);
}

#[test]
fn migration_takes_committed_data_from_a_wal_file_and_leaves_old_files_alone() {
    let home = tempfile::tempdir().unwrap();
    let old = old_macos(home.path());
    // The connection stays open: the row only lives in `cockpit.db-wal`.
    let _open = make_db(&old.join("cockpit.db"), true);
    let wal = old.join("cockpit.db-wal");
    assert!(wal.is_file(), "test setup: no -wal file");
    let wal_before = std::fs::metadata(&wal).unwrap().len();
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[("HOME", s(home.path()))]);
    let notes = prepare(project.path(), &env, true).unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(rows(&project.path().join("config/cockpit.db")), ["old row"]);
    assert!(wal.is_file());
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), wal_before);
}

#[test]
fn migration_works_with_the_linux_layout() {
    let home = tempfile::tempdir().unwrap();
    let cfg = home.path().join("xdg-config/cockpit");
    let data = home.path().join("xdg-data/cockpit");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(cfg.join("config.toml"), "port = 9002\n").unwrap();
    drop(make_db(&data.join("cockpit.db"), false));
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[
        ("HOME", s(home.path())),
        ("XDG_CONFIG_HOME", s(&home.path().join("xdg-config"))),
        ("XDG_DATA_HOME", s(&home.path().join("xdg-data"))),
    ]);
    assert_eq!(prepare(project.path(), &env, false).unwrap().len(), 2);
    assert_eq!(
        std::fs::read_to_string(project.path().join("config/config.toml")).unwrap(),
        "port = 9002\n"
    );
    assert_eq!(rows(&project.path().join("config/cockpit.db")), ["old row"]);
}

#[test]
fn without_old_files_only_the_empty_config_is_created() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[("HOME", s(home.path()))]);
    prepare(project.path(), &env, true).unwrap();
    assert!(!project.path().join("config/cockpit.db").exists());
}

#[cfg(unix)]
#[test]
fn tighten_db_sets_600() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("cockpit.db");
    std::fs::write(&db, "").unwrap();
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    tighten_db(&db);
    assert_eq!(mode(&db), 0o600);
}

#[test]
fn config_example_parses_strictly_and_covers_all_sections() {
    let text = include_str!("../../../../../config/config.example.toml");
    // Strict: unknown keys are an error (`deny_unknown_fields`).
    let c = crate::config::Config::parse(Some(text), |_| None).expect("example must parse");
    assert!(c.github.as_ref().is_some_and(|g| !g.repos.is_empty()));
    assert!(c.jira.is_some());
    assert!(c.github_oauth.is_some());
    assert!(c.atlassian.is_some());
    assert!(c.endpoint.is_some());
    assert!(c.container.is_some());
    assert!(c.agent_repo.is_some());
    assert_eq!(c.agent_repos.len(), 2);
    assert!(c.public_url.is_some());
    assert!(c.webhook_fallback_interval_s.is_some());
    // Every section of the file format appears as an active table.
    let table = |name: &str| text.lines().any(|l| l.trim_end() == format!("[{name}]"));
    for section in [
        "github",
        "jira",
        "github_oauth",
        "atlassian",
        "agent",
        "agent.endpoint",
        "server",
        "sync",
    ] {
        assert!(table(section), "[{section}] missing in config.example.toml");
    }
    assert!(text.contains("[[agent.repos]]"));
    assert!(text.contains("interval_s") && text.contains("port"));
    // Dummy values only.
    for real in ["atlassian.net", "@gmail", "ghp_", "github_pat_"] {
        assert!(!text.contains(real), "{real}");
    }
    assert!(text.contains("you@example.com") && text.contains("acme/app"));
}

#[test]
fn config_example_is_not_ignored_but_the_real_files_are() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ok = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .ok()
    };
    let Some(inside) = ok(&["rev-parse", "--is-inside-work-tree"]) else {
        return;
    };
    if !inside.status.success() {
        return;
    }
    let ignored = |f: &str| {
        ok(&["check-ignore", "-q", "--", f])
            .unwrap()
            .status
            .success()
    };
    assert!(ignored("config/config.toml"));
    assert!(ignored("config/cockpit.db"));
    assert!(ignored("config/cockpit.db-wal"));
    assert!(!ignored("config/config.example.toml"));
}

/// The shell part of the CI job `no-env-files`, run against temporary repositories.
#[test]
fn ci_script_fails_for_tracked_config_files_only() {
    use std::process::Command;
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/check-tracked-files.sh");
    let git = |dir: &Path, args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status();
        match st {
            Ok(s) => s.success(),
            Err(_) => false,
        }
    };
    let dir = tempfile::tempdir().unwrap();
    if !git(dir.path(), &["init", "-q"]) {
        return; // no git here
    }
    let run = |dir: &Path| Command::new("bash").arg(&script).arg(dir).output().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(dir.path().join(".env.example"), "A=dummy\n").unwrap();
    std::fs::write(dir.path().join("config/config.example.toml"), "port = 1\n").unwrap();
    assert!(git(dir.path(), &["add", "-f", "."]));
    assert!(
        run(dir.path()).status.success(),
        "example files are allowed"
    );

    std::fs::write(dir.path().join("config/config.toml"), "port = 2\n").unwrap();
    assert!(git(dir.path(), &["add", "-f", "config/config.toml"]));
    let out = run(dir.path());
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("config/config.toml"));

    assert!(git(
        dir.path(),
        &["rm", "-q", "--cached", "config/config.toml"]
    ));
    std::fs::write(dir.path().join("config/cockpit.db"), "x").unwrap();
    assert!(git(dir.path(), &["add", "-f", "config/cockpit.db"]));
    assert!(!run(dir.path()).status.success());
    assert!(git(
        dir.path(),
        &["rm", "-q", "--cached", "config/cockpit.db"]
    ));
    assert!(run(dir.path()).status.success());

    // `.env` is still caught.
    std::fs::write(dir.path().join(".env"), "A=1\n").unwrap();
    assert!(git(dir.path(), &["add", "-f", ".env"]));
    assert!(!run(dir.path()).status.success());
}
