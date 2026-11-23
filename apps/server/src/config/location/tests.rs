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

#[cfg(unix)]
#[test]
fn first_start_creates_the_folder_and_an_empty_config_with_tight_modes() {
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[]);
    prepare(project.path(), &env).unwrap();
    let dir = project.path().join("config");
    assert_eq!(mode(&dir), 0o700);
    let file = dir.join("config.toml");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "");
    assert_eq!(mode(&file), 0o600);
    // A second start leaves everything as it is.
    std::fs::write(&file, "port = 1234\n").unwrap();
    prepare(project.path(), &env).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "port = 1234\n");
}

#[test]
fn both_overrides_create_nothing() {
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[
        ("COCKPIT_CONFIG", "/somewhere/c.toml".into()),
        ("COCKPIT_DB", "/somewhere/c.db".into()),
    ]);
    prepare(project.path(), &env).unwrap();
    assert!(!project.path().join("config").exists());
}

#[test]
fn an_explicit_config_path_is_never_created() {
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[("COCKPIT_CONFIG", s(&project.path().join("mine.toml")))]);
    prepare(project.path(), &env).unwrap();
    assert!(!project.path().join("mine.toml").exists());
    assert!(!project.path().join("config/config.toml").exists());
}

#[test]
fn an_explicit_db_path_still_gets_the_folder_and_an_empty_config() {
    let project = tempfile::tempdir().unwrap();
    let env = env_of(&[("COCKPIT_DB", s(&project.path().join("x.db")))]);
    prepare(project.path(), &env).unwrap();
    assert_eq!(
        std::fs::read_to_string(project.path().join("config/config.toml")).unwrap(),
        ""
    );
    assert!(!project.path().join("config/cockpit.db").exists());
}

#[test]
fn only_the_empty_config_is_created_never_a_database() {
    let project = tempfile::tempdir().unwrap();
    prepare(project.path(), &env_of(&[])).unwrap();
    assert!(project.path().join("config/config.toml").is_file());
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
    std::fs::write(dir.path().join("config/config.example.toml"), "port = 1\n").unwrap();
    assert!(git(dir.path(), &["add", "-f", "."]));
    assert!(
        run(dir.path()).status.success(),
        "the example file is allowed"
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
