use super::*;

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn run(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

#[cfg(unix)]
fn chmod(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A repository with the repo's ignore rules and a `config/` folder with tight modes.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init", "-q"]);
    std::fs::write(
        dir.path().join(".gitignore"),
        "config/*\n!config/config.example.toml\n",
    )
    .unwrap();
    let config = dir.path().join("config");
    std::fs::create_dir(&config).unwrap();
    #[cfg(unix)]
    chmod(&config, 0o700);
    (dir, config)
}

fn write(dir: &Path, name: &str, mode: u32) {
    let p = dir.join(name);
    std::fs::write(&p, "x").unwrap();
    #[cfg(unix)]
    chmod(&p, mode);
    #[cfg(not(unix))]
    let _ = mode;
}

#[test]
fn clean_folder_has_no_warnings() {
    if !git_available() {
        return;
    }
    let (_repo, config) = repo();
    write(&config, "config.toml", 0o600);
    write(&config, "cockpit.db", 0o600);
    write(&config, "cockpit.db-wal", 0o600);
    assert!(check(&config, Language::German).is_empty());
    assert!(check(&config, Language::English).is_empty());
}

#[test]
fn a_tracked_config_file_is_reported_in_both_languages() {
    if !git_available() {
        return;
    }
    let (repo, config) = repo();
    write(&config, "config.toml", 0o600);
    run(repo.path(), &["add", "-f", "config/config.toml"]);
    let de = check(&config, Language::German);
    assert_eq!(de.len(), 1, "{de:?}");
    assert!(
        de[0].contains("wird von Git verfolgt")
            && de[0].contains("git rm --cached config/config.toml")
    );
    let en = check(&config, Language::English);
    assert!(en[0].contains("is tracked by Git"));
}

#[test]
fn the_example_file_may_be_tracked() {
    if !git_available() {
        return;
    }
    let (repo, config) = repo();
    write(&config, "config.example.toml", 0o644);
    write(&config, "config.toml", 0o600);
    run(repo.path(), &["add", "config/config.example.toml"]);
    assert!(check(&config, Language::German).is_empty());
}

#[test]
fn a_file_that_git_does_not_ignore_is_reported() {
    if !git_available() {
        return;
    }
    let (repo, config) = repo();
    std::fs::write(repo.path().join(".gitignore"), "").unwrap();
    write(&config, "config.toml", 0o600);
    let w = check(&config, Language::English);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("not protected by .gitignore"));
    assert!(check(&config, Language::German)[0].contains("nicht durch .gitignore geschützt"));
}

#[test]
fn outside_a_repository_nothing_is_said_about_git() {
    if !git_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    if git(dir.path(), &["rev-parse", "--is-inside-work-tree"]).is_some_and(|r| r.0) {
        return;
    }
    let config = dir.path().join("config");
    std::fs::create_dir(&config).unwrap();
    write(&config, "config.toml", 0o600);
    #[cfg(unix)]
    chmod(&config, 0o700);
    assert!(check(&config, Language::German).is_empty());
}

#[cfg(unix)]
#[test]
fn loose_modes_are_reported_and_fixed_at_startup() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    std::fs::create_dir(&config).unwrap();
    chmod(&config, 0o755);
    write(&config, "config.toml", 0o644);
    write(&config, "config.example.toml", 0o644);
    let w = check(&config, Language::German);
    // Folder and config.toml, not the example.
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(
        w.iter()
            .all(|m| m.contains("für andere Benutzer zugänglich"))
    );
    assert!(!w.iter().any(|m| m.contains("example")));

    let notes = check_and_fix(&config);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes.iter().all(|n| n.contains("was set to")));
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&config), 0o700);
    assert_eq!(mode(&config.join("config.toml")), 0o600);
    assert_eq!(mode(&config.join("config.example.toml")), 0o644);
    assert!(check(&config, Language::German).is_empty());
}

#[test]
fn legacy_notice_lasts_while_old_files_exist_and_touches_nothing() {
    let old = tempfile::tempdir().unwrap();
    let files = vec![
        old.path().join("config.toml"),
        old.path().join("cockpit.db"),
    ];
    assert!(legacy_notices(&files).is_empty());
    std::fs::write(&files[0], "port = 1\n").unwrap();
    std::fs::write(&files[1], "db").unwrap();
    let n = legacy_notices(&files);
    assert_eq!(n.len(), 1);
    assert!(n[0].contains("Alte Konfiguration liegt noch unter"));
    assert!(n[0].contains(&old.path().display().to_string()));
    assert!(n[0].contains("du kannst sie löschen"));
    assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), "port = 1\n");
    std::fs::remove_file(&files[0]).unwrap();
    std::fs::remove_file(&files[1]).unwrap();
    assert!(legacy_notices(&files).is_empty());
}

#[test]
fn status_names_the_folder_and_carries_notices() {
    let dir = tempfile::tempdir().unwrap();
    let st = status(dir.path(), &[], None);
    assert_eq!(st.path, dir.path().display().to_string());
    assert!(st.notices.is_empty());
}

#[test]
fn old_env_file_notice_lasts_while_the_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join(".env");
    assert!(status(dir.path(), &[], Some(&old)).notice_msgs.is_empty());
    std::fs::write(&old, "JIRA_TOKEN=dummy-token\n").unwrap();
    let st = status(dir.path(), &[], Some(&old));
    assert_eq!(st.notices.len(), 1);
    assert!(st.notices[0].contains("config/.env"), "{:?}", st.notices);
    assert!(st.notices[0].contains(&old.display().to_string()));
    assert!(old.exists());
}
