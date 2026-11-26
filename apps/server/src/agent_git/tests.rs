use super::*;
use cockpit_core::RepoRules;
use cockpit_i18n::Lang;

#[test]
fn addresses() {
    assert_eq!(
        base_from_api("https://api.github.com/"),
        "https://github.com"
    );
    assert_eq!(
        base_from_api("https://ghe.example/api/v3"),
        "https://ghe.example"
    );
    assert_eq!(
        clone_url("https://github.com/", "firma/app"),
        "https://github.com/firma/app.git"
    );
}

#[test]
fn base_64_matches() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}

#[test]
fn the_token_goes_only_at_https_and_only_via_the_env() {
    let t = Secret::new("ghp_geheim");
    assert!(auth_env("file:///tmp/x.git", Some(&t)).is_empty());
    assert!(auth_env("http://github.com/x.git", Some(&t)).is_empty());
    assert!(auth_env("https://github.com/x.git", None).is_empty());
    let env = auth_env("https://github.com/x.git", Some(&t));
    assert_eq!(env.len(), 3);
    assert!(env[2].1.starts_with("Authorization: Basic "));
    assert!(!env[2].1.contains("ghp_geheim"), "only base64-encoded");
}

#[test]
fn error_texts_contained_no_token() {
    let t = "ghp_geheim123";
    let b = base64(format!("x-access-token:{t}").as_bytes());
    let text = format!("fatal: https://x-access-token:{t}@github.com und {b}");
    let s = redact(&text, &[t]);
    assert!(!s.contains(t) && !s.contains(&b), "{s}");
}

/// A remote with a commit on `main`, a clone folder and a folder for the export.
struct Layout {
    _tmp: tempfile::TempDir,
    remote: std::path::PathBuf,
    clone: std::path::PathBuf,
    export: std::path::PathBuf,
    url: String,
}

fn g(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .args([] as [&str; 0])
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@t.invalid")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@t.invalid")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn layout() -> Layout {
    let tmp = tempfile::tempdir().unwrap();
    let remote = tmp.path().join("remote.git");
    let source = tmp.path().join("quelle");
    std::fs::create_dir_all(&source).unwrap();
    g(&source, &["init", "--quiet", "-b", "main"]);
    std::fs::write(source.join("README.md"), "hallo\n").unwrap();
    g(&source, &["add", "-A"]);
    g(&source, &["commit", "--quiet", "-m", "Anfang"]);
    g(
        tmp.path(),
        &["clone", "--quiet", "--bare", "quelle", "remote.git"],
    );
    Layout {
        url: format!("file://{}", remote.display()),
        clone: tmp.path().join("klon"),
        export: tmp.path().join("export.git"),
        remote,
        _tmp: tmp,
    }
}

fn heads(remote: &Path) -> Vec<String> {
    g(
        remote,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .lines()
    .map(str::to_string)
    .collect()
}

/// Imitates the container: changes files in the clone and commits.
fn agent_works(clone: &Path, file: &str) {
    let path = clone.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "neu\n").unwrap();
    g(clone, &["add", "-A"]);
    g(clone, &["commit", "--quiet", "-m", "Agent"]);
}

/// A new branch under the default pattern, off `main`.
fn rule(branch: &str) -> BranchRule<'_> {
    BranchRule {
        branch,
        pattern: cockpit_core::branch::DEFAULT_PATTERN,
        base: "main",
        existing: false,
    }
}

/// A follow-up on an existing branch.
fn follow(branch: &str) -> BranchRule<'_> {
    BranchRule {
        existing: true,
        ..rule(branch)
    }
}

#[test]
fn clone_work_check_push_only_on_pattern_branches() {
    let a = layout();
    let base = clone(&a.url, &a.clone, None, &rule("claude/BAU-1-x")).unwrap();
    assert_eq!(base.len(), 40);
    assert_eq!(
        g(&a.clone, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "claude/BAU-1-x"
    );
    assert_eq!(
        g(&a.clone, &["remote"]).trim(),
        "",
        "no remote in the clone"
    );
    agent_works(&a.clone, "src/neu.rs");
    let v = publish(
        &a.clone,
        &a.export,
        &base,
        &a.url,
        None,
        &RepoRules::default(),
        &rule("claude/BAU-1-x"),
    )
    .unwrap();
    assert_eq!(v.files, ["src/neu.rs"]);
    let mut h = heads(&a.remote);
    h.sort();
    assert_eq!(h, ["claude/BAU-1-x", "main"]);
    // main in the remote is untouched.
    assert_eq!(g(&a.remote, &["rev-parse", "main"]).trim(), base);
    assert_eq!(
        g(&a.remote, &["rev-parse", "claude/BAU-1-x"]).trim(),
        v.commit
    );
}

#[test]
fn the_main_branch_is_never_created_or_pushed() {
    let a = layout();
    assert!(clone(&a.url, &a.clone, None, &rule("main")).is_err());
    let base = clone(&a.url, &a.clone, None, &rule("claude/BAU-2-x")).unwrap();
    agent_works(&a.clone, "a.txt");
    for b in ["main", "master", "agent/x", "refs/heads/main"] {
        let e = publish(
            &a.clone,
            &a.export,
            &base,
            &a.url,
            None,
            &RepoRules::default(),
            &rule(b),
        );
        assert!(e.is_err(), "{b}");
    }
    assert_eq!(heads(&a.remote), ["main"]);
}

#[test]
fn rules_the_repos_prevent_the_push() {
    let a = layout();
    let base = clone(&a.url, &a.clone, None, &rule("claude/BAU-3-r")).unwrap();
    agent_works(&a.clone, "Cargo.lock");
    let rules = RepoRules {
        forbidden_paths: vec!["*.lock".into()],
        ..RepoRules::default()
    };
    let e = publish(
        &a.clone,
        &a.export,
        &base,
        &a.url,
        None,
        &rules,
        &rule("claude/BAU-3-r"),
    )
    .unwrap_err()
    .render(Lang::De);
    assert!(e.contains("verstoßen gegen die Regeln"), "{e}");
    assert_eq!(heads(&a.remote), ["main"]);
}

#[test]
fn without_commit_and_with_foreign_history_is_not_pushed() {
    let a = layout();
    let base = clone(&a.url, &a.clone, None, &rule("claude/BAU-4-leer")).unwrap();
    let e = publish(
        &a.clone,
        &a.export,
        &base,
        &a.url,
        None,
        &RepoRules::default(),
        &rule("claude/BAU-4-leer"),
    )
    .unwrap_err()
    .render(Lang::De);
    assert!(e.contains("nichts committet"), "{e}");

    // Rewritten history: a branch without the starting state.
    let b = layout();
    let base = clone(&b.url, &b.clone, None, &rule("claude/BAU-5-fremd")).unwrap();
    g(
        &b.clone,
        &["checkout", "--quiet", "--orphan", "claude/BAU-5-fremd2"],
    );
    agent_works(&b.clone, "x.txt");
    let e = publish(
        &b.clone,
        &b.export,
        &base,
        &b.url,
        None,
        &RepoRules::default(),
        &rule("claude/BAU-5-fremd2"),
    )
    .unwrap_err()
    .render(Lang::De);
    assert!(e.contains("baut nicht auf"), "{e}");
    assert_eq!(heads(&b.remote), ["main"]);
}

#[cfg(unix)]
#[test]
fn a_tampered_clone_leads_on_the_machine_nothing_out() {
    use std::os::unix::fs::PermissionsExt;
    let a = layout();
    let base = clone(&a.url, &a.clone, None, &rule("claude/BAU-6-boese")).unwrap();
    let mark = a.clone.parent().unwrap().join("ausgefuehrt");
    // Hooks and commands in the configuration, as an agent could put them in .git.
    for hook in [
        "pre-push",
        "post-commit",
        "reference-transaction",
        "pre-commit",
    ] {
        let p = a.clone.join(".git/hooks").join(hook);
        std::fs::write(&p, format!("#!/bin/sh\ntouch {}\n", mark.display())).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut config = std::fs::read_to_string(a.clone.join(".git/config")).unwrap();
    config.push_str(&format!(
        "[core]\n\tfsmonitor = touch {m}\n\tsshCommand = touch {m}\n[credential]\n\thelper = !touch {m}\n",
        m = mark.display()
    ));
    std::fs::write(a.clone.join(".git/config"), config).unwrap();
    std::fs::write(a.clone.join("x.txt"), "x").unwrap();
    // The "agent" commits past the hooks (otherwise the hook would run in the test itself).
    g(&a.clone, &["add", "-A"]);
    g(
        &a.clone,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "x",
        ],
    );
    let _ = std::fs::remove_file(&mark);
    publish(
        &a.clone,
        &a.export,
        &base,
        &a.url,
        None,
        &RepoRules::default(),
        &rule("claude/BAU-6-boese"),
    )
    .unwrap();
    assert!(!mark.exists(), "nothing may run in the clone");
    assert_eq!(heads(&a.remote).len(), 2);
}

#[test]
fn clone_fails_in_plain_text() {
    let a = layout();
    let e = clone(
        "file:///gibt/es/nicht.git",
        &a.clone,
        None,
        &rule("claude/BAU-2-x"),
    )
    .unwrap_err()
    .render(Lang::De);
    assert!(!e.is_empty());
    let evil = BranchRule {
        base: "--evil",
        ..rule("claude/BAU-2-x")
    };
    assert!(clone(&a.url, &a.clone, None, &evil).is_err());
}

#[test]
fn a_followup_continues_on_the_existing_branch_and_pushes_normally() {
    let a = layout();
    // The earlier run: branch with one commit in the remote.
    let first = clone(&a.url, &a.clone, None, &rule("claude/BAU-1-x")).unwrap();
    agent_works(&a.clone, "alt.txt");
    let earlier = publish(
        &a.clone,
        &a.export,
        &first,
        &a.url,
        None,
        &RepoRules::default(),
        &rule("claude/BAU-1-x"),
    )
    .unwrap();
    // The follow-up starts from the branch, not from main.
    struct Again {
        clone: std::path::PathBuf,
        export: std::path::PathBuf,
    }
    let dir = a.remote.parent().unwrap();
    let again = Again {
        clone: dir.join("klon2"),
        export: dir.join("export2.git"),
    };
    let base = clone_branch(&a.url, &again.clone, None, &follow("claude/BAU-1-x")).unwrap();
    assert_eq!(base, earlier.commit);
    assert_eq!(
        g(&again.clone, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "claude/BAU-1-x"
    );
    assert_eq!(
        g(&again.clone, &["remote"]).trim(),
        "",
        "no remote in the clone"
    );
    agent_works(&again.clone, "neu.txt");
    let v = publish(
        &again.clone,
        &again.export,
        &base,
        &a.url,
        None,
        &RepoRules::default(),
        &follow("claude/BAU-1-x"),
    )
    .unwrap();
    // Only the new file counts as changed, the history was kept (fast-forward).
    assert_eq!(v.files, ["neu.txt"]);
    let log = g(&a.remote, &["log", "--format=%s", "claude/BAU-1-x"]);
    assert_eq!(log.lines().count(), 3, "{log}");
    g(
        &a.remote,
        &["merge-base", "--is-ancestor", &earlier.commit, &v.commit],
    );
}

#[test]
fn a_followup_never_clones_a_foreign_branch() {
    let a = layout();
    for bad in [
        "main",
        "feature/x",
        "claude/../main",
        "claude/a;b",
        "develop",
    ] {
        let e = clone_branch(&a.url, &a.clone, None, &follow(bad))
            .unwrap_err()
            .render(Lang::De);
        assert!(!e.is_empty(), "{bad}");
    }
    // A branch that does not exist: a plain error, nothing is created.
    assert!(
        clone_branch(
            &a.url,
            &a.clone,
            None,
            &follow("claude/BAU-9-gibt-es-nicht")
        )
        .is_err()
    );
}

mod patterns;
