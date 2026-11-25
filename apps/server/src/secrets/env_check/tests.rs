use super::*;
use cockpit_i18n::Lang;
use std::process::Command;

/// The German texts of warnings.
fn german(warnings: Vec<Msg>) -> Vec<String> {
    warnings.iter().map(|m| m.render(Lang::De)).collect()
}

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

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init", "-q"]);
    dir
}

#[test]
fn outside_a_repository_nothing_is_said() {
    if !git_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // The temp folder is not inside a repository (checked, else skip).
    if git(dir.path(), &["rev-parse", "--is-inside-work-tree"]).is_some_and(|r| r.0) {
        return;
    }
    let path = dir.path().join(".env");
    std::fs::write(&path, "JIRA_TOKEN=dummy\n").unwrap();
    assert!(german(git_messages(&path)).is_empty());
}

#[test]
fn not_ignored_is_reported() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let path = dir.path().join(".env");
    std::fs::write(&path, "JIRA_TOKEN=dummy\n").unwrap();
    let w = german(git_messages(&path));
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("nicht durch .gitignore geschützt"));
    assert!(!w[0].contains("dummy"));
}

#[test]
fn ignored_file_gives_no_warning() {
    if !git_available() {
        return;
    }
    let dir = repo();
    std::fs::write(dir.path().join(".gitignore"), ".env\n").unwrap();
    let path = dir.path().join(".env");
    std::fs::write(&path, "JIRA_TOKEN=dummy\n").unwrap();
    assert!(german(git_messages(&path)).is_empty());
    // Also before the file exists.
    std::fs::remove_file(&path).unwrap();
    assert!(german(git_messages(&path)).is_empty());
}

#[test]
fn tracked_file_is_reported_even_if_ignored() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let path = dir.path().join(".env");
    std::fs::write(&path, "JIRA_TOKEN=dummy\n").unwrap();
    run(dir.path(), &["add", ".env"]);
    std::fs::write(dir.path().join(".gitignore"), ".env\n").unwrap();
    let w = german(git_messages(&path));
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("von Git verfolgt"));
    assert!(w[0].contains("git rm --cached .env"));
}

#[test]
fn env_files_are_ignored_by_the_repo_gitignore() {
    if !git_available() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ignored = |f: &str| git(&root, &["check-ignore", "-q", "--", f]).is_some_and(|r| r.0);
    if git(&root, &["rev-parse", "--is-inside-work-tree"]).is_none_or(|r| !r.0) {
        return;
    }
    assert!(ignored(".env"));
    assert!(ignored(".env.local"));
    assert!(ignored("apps/server/.env"));
}

#[cfg(unix)]
#[test]
fn loose_permissions_are_reported_and_fixed_at_startup() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    std::fs::write(&path, "JIRA_TOKEN=dummy\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let w = check(&path);
    assert!(
        w.iter()
            .any(|m| m.render(Lang::De).contains("lesbar (Rechte 644)")),
        "{w:?}"
    );
    // `check` changes nothing.
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    let notes = check_and_fix(&path);
    assert!(notes.iter().any(|m| m.contains("set to 600")), "{notes:?}");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        check(&path)
            .iter()
            .all(|m| !m.render(Lang::De).contains("Rechte"))
    );
}

#[cfg(unix)]
#[test]
fn tight_permissions_and_missing_file_are_quiet() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    assert!(loose_mode(&path).is_none());
    std::fs::write(&path, "").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(loose_mode(&path).is_none());
}

#[test]
fn status_names_the_path_and_never_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    std::fs::write(&path, "JIRA_TOKEN=dummy-geheim\n").unwrap();
    let s = status(&path);
    assert_eq!(s.path, path.display().to_string());
    assert!(!format!("{s:?}").contains("dummy-geheim"));
}
