use super::*;
use crate::secrets::MemoryStore;

fn store_in(dir: &Path) -> EnvFileStore {
    EnvFileStore::with_env(dir.join(".env"), |_| None)
}

fn read(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(".env")).unwrap()
}

fn value(s: &EnvFileStore, name: &str) -> Option<String> {
    s.get(name).unwrap().map(|v| v.expose().to_string())
}

#[test]
fn set_get_replace_delete() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_in(dir.path());
    assert_eq!(value(&s, GITHUB), None);
    assert!(!dir.path().join(".env").exists());
    s.set(GITHUB, &Secret::new("dummy-github")).unwrap();
    s.set(JIRA, &Secret::new("dummy-jira")).unwrap();
    assert_eq!(value(&s, GITHUB).as_deref(), Some("dummy-github"));
    assert!(s.has(JIRA).unwrap());
    s.set(GITHUB, &Secret::new("dummy-github-2")).unwrap();
    assert_eq!(value(&s, GITHUB).as_deref(), Some("dummy-github-2"));
    // Replaced, not appended.
    assert_eq!(
        read(dir.path()),
        "GITHUB_TOKEN=dummy-github-2\nJIRA_TOKEN=dummy-jira\n"
    );
    s.delete(GITHUB).unwrap();
    s.delete(GITHUB).unwrap(); // deleting twice is no error
    assert_eq!(value(&s, GITHUB), None);
    assert_eq!(read(dir.path()), "JIRA_TOKEN=dummy-jira\n");
}

#[test]
fn delete_without_file_is_fine_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    store_in(dir.path()).delete(JIRA).unwrap();
    assert!(!dir.path().join(".env").exists());
}

#[test]
fn comments_and_foreign_lines_stay() {
    let dir = tempfile::tempdir().unwrap();
    let before =
        "# my notes\n\nOTHER_TOOL=1\nJIRA_TOKEN=old # inline\n  # indented\nexport KEEP='x y'\n";
    std::fs::write(dir.path().join(".env"), before).unwrap();
    let s = store_in(dir.path());
    assert_eq!(value(&s, JIRA).as_deref(), Some("old"));
    s.set(JIRA, &Secret::new("new")).unwrap();
    s.set(GITHUB, &Secret::new("gh")).unwrap();
    assert_eq!(
        read(dir.path()),
        "# my notes\n\nOTHER_TOOL=1\nJIRA_TOKEN=new\n  # indented\nexport KEEP='x y'\nGITHUB_TOKEN=gh\n"
    );
    s.delete(JIRA).unwrap();
    assert_eq!(
        read(dir.path()),
        "# my notes\n\nOTHER_TOOL=1\n  # indented\nexport KEEP='x y'\nGITHUB_TOKEN=gh\n"
    );
}

#[test]
fn append_after_missing_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), "# c").unwrap();
    store_in(dir.path()).set(JIRA, &Secret::new("t")).unwrap();
    assert_eq!(read(dir.path()), "# c\nJIRA_TOKEN=t\n");
}

#[test]
fn duplicates_collapse_to_one_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), "JIRA_TOKEN=a\nX=1\nJIRA_TOKEN=b\n").unwrap();
    let s = store_in(dir.path());
    assert_eq!(value(&s, JIRA).as_deref(), Some("b")); // the last one counts
    s.set(JIRA, &Secret::new("c")).unwrap();
    assert_eq!(read(dir.path()), "JIRA_TOKEN=c\nX=1\n");
}

#[test]
fn special_characters_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_in(dir.path());
    let values = [
        "a=b==c",
        "with space and # hash",
        "dollar $HOME `cmd` \\ back",
        "it's \"quoted\"",
        "line1\nline2\r\nline3",
        "  padded  ",
        "ümlaut €",
        "#startswith",
        "=lead",
    ];
    for v in values {
        s.set(JIRA, &Secret::new(v)).unwrap();
        assert_eq!(value(&s, JIRA).as_deref(), Some(v), "{v:?}");
        // The file stays one line per entry.
        assert_eq!(read(dir.path()).lines().count(), 1, "{v:?}");
    }
}

#[test]
fn json_blob_stays_on_one_line_and_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_in(dir.path());
    let blob = r#"{"access":"dummy-access","refresh":"dummy-refresh","expires":1893456000}"#;
    s.set(GITHUB_OAUTH_TOKENS, &Secret::new(blob)).unwrap();
    s.set(ATLASSIAN_OAUTH_TOKENS, &Secret::new(blob)).unwrap();
    assert_eq!(
        read(dir.path()),
        format!("GITHUB_OAUTH_TOKENS='{blob}'\nATLASSIAN_OAUTH_TOKENS='{blob}'\n")
    );
    assert_eq!(value(&s, GITHUB_OAUTH_TOKENS).as_deref(), Some(blob));
    let parsed: serde_json::Value =
        serde_json::from_str(&value(&s, ATLASSIAN_OAUTH_TOKENS).unwrap()).unwrap();
    assert_eq!(parsed["refresh"], "dummy-refresh");
}

#[test]
fn double_quoted_escapes_are_read() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".env"),
        "JIRA_TOKEN=\"a\\\"b\\\\c\\nd\" # note\nGITHUB_TOKEN=\"\"\n",
    )
    .unwrap();
    let s = store_in(dir.path());
    assert_eq!(value(&s, JIRA).as_deref(), Some("a\"b\\c\nd"));
    // An empty value counts as not set.
    assert_eq!(value(&s, GITHUB), None);
}

#[test]
fn process_environment_wins_over_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let s = EnvFileStore::with_env(dir.path().join(".env"), |k| {
        (k == "JIRA_TOKEN").then(|| "from-process".to_string())
    });
    s.set(JIRA, &Secret::new("from-file")).unwrap();
    s.set(GITHUB, &Secret::new("gh-file")).unwrap();
    assert_eq!(value(&s, JIRA).as_deref(), Some("from-process"));
    assert_eq!(value(&s, GITHUB).as_deref(), Some("gh-file"));
    // Without a process variable the file is read.
    let s2 = store_in(dir.path());
    assert_eq!(value(&s2, JIRA).as_deref(), Some("from-file"));
}

#[test]
fn old_name_litellm_reads_agent_key() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_in(dir.path());
    s.set(AGENT_KEY, &Secret::new("dummy-agent")).unwrap();
    assert_eq!(read(dir.path()), "AGENT_KEY=dummy-agent\n");
    assert_eq!(value(&s, LITELLM).as_deref(), Some("dummy-agent"));
    assert_eq!(variable_for(LITELLM), variable_for(AGENT_KEY));
    // The fallback of the backend still finds the key.
    let found = crate::app::agent_key_read(&s).unwrap().unwrap();
    assert_eq!(found.expose(), "dummy-agent");
}

#[test]
fn unknown_names_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_in(dir.path());
    assert!(s.get("nope").is_err());
    assert!(s.set("nope", &Secret::new("x")).is_err());
    assert!(!dir.path().join(".env").exists());
}

#[cfg(unix)]
#[test]
fn file_is_created_and_kept_with_mode_600() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let s = store_in(dir.path());
    s.set(JIRA, &Secret::new("a")).unwrap();
    assert_eq!(mode(&path), 0o600);
    // A loose file gets tight again with the next write (new file replaces the old one).
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    s.set(JIRA, &Secret::new("b")).unwrap();
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn writing_is_atomic_no_temp_files_remain() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_in(dir.path());
    for i in 0..5 {
        s.set(JIRA, &Secret::new(format!("v{i}"))).unwrap();
    }
    s.delete(JIRA).unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![".env".to_string()]);
}

#[cfg(unix)]
#[test]
fn failed_write_keeps_the_old_file_and_removes_the_temp_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let s = store_in(dir.path());
    s.set(JIRA, &Secret::new("keep")).unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let writable = std::fs::File::create(dir.path().join("probe")).is_ok();
    let result = s.set(JIRA, &Secret::new("lost"));
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    if writable {
        return; // running as root: the folder stays writable, nothing to check
    }
    assert!(result.is_err());
    assert_eq!(read(dir.path()), "JIRA_TOKEN=keep\n");
}

#[test]
fn symlink_is_followed_and_kept() {
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.env");
        std::fs::write(&real, "A=1\n").unwrap();
        let link = dir.path().join(".env");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let s = store_in(dir.path());
        s.set(JIRA, &Secret::new("t")).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            std::fs::read_to_string(&real).unwrap(),
            "A=1\nJIRA_TOKEN=t\n"
        );
    }
}

#[test]
fn no_values_in_debug_or_errors() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_in(dir.path());
    s.set(JIRA, &Secret::new("dummy-geheim-123")).unwrap();
    let out = format!("{s:?} {:?}", s.get(JIRA).unwrap());
    assert!(!out.contains("dummy-geheim-123"), "{out}");
    assert!(out.contains("Secret(***)"));
    // An unreadable file (a folder instead of a file) gives an error without any content.
    let d2 = tempfile::tempdir().unwrap();
    std::fs::create_dir(d2.path().join(".env")).unwrap();
    let err = store_in(d2.path()).get(JIRA).unwrap_err().to_string();
    assert!(err.contains(".env"));
    let err = store_in(d2.path())
        .set(JIRA, &Secret::new("dummy-geheim-123"))
        .unwrap_err()
        .to_string();
    assert!(!err.contains("dummy-geheim-123"), "{err}");
}

#[test]
fn default_path_prefers_the_variable() {
    assert_eq!(
        default_path(|k| (k == ENV_FILE_VAR).then(|| "/x/y.env".to_string())),
        PathBuf::from("/x/y.env")
    );
    assert_eq!(
        default_path(|_| None).file_name().unwrap(),
        std::ffi::OsStr::new(".env")
    );
    assert_eq!(
        default_path(|_| Some(String::new())).file_name().unwrap(),
        std::ffi::OsStr::new(".env")
    );
}

#[test]
fn take_over_copies_but_never_overwrites_or_deletes() {
    let source = MemoryStore::new();
    source.set(JIRA, &Secret::new("dummy-old-jira")).unwrap();
    source.set(GITHUB, &Secret::new("dummy-old-gh")).unwrap();
    source.set(LITELLM, &Secret::new("dummy-old-llm")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = store_in(dir.path());
    target.set(GITHUB, &Secret::new("dummy-new-gh")).unwrap();
    let report = take_over(&source, &target).unwrap();
    let outcome = |n: &str| report.iter().find(|(m, _)| *m == n).unwrap().1;
    assert_eq!(outcome(JIRA), Takeover::Taken);
    assert_eq!(outcome(GITHUB), Takeover::AlreadyPresent);
    assert_eq!(outcome(ATLASSIAN_OAUTH), Takeover::NotFound);
    assert_eq!(value(&target, JIRA).as_deref(), Some("dummy-old-jira"));
    assert_eq!(value(&target, GITHUB).as_deref(), Some("dummy-new-gh"));
    assert_eq!(value(&target, AGENT_KEY).as_deref(), Some("dummy-old-llm"));
    // The source is untouched.
    assert_eq!(
        source.get(JIRA).unwrap().unwrap().expose(),
        "dummy-old-jira"
    );
    assert!(source.has(GITHUB).unwrap());
}

/// `.env.example` lists every variable of the code, with dummy values only.
#[test]
fn env_example_lists_all_names_with_dummy_values() {
    let example = include_str!("../../../../../.env.example");
    let mut seen = std::collections::HashMap::new();
    for line in example.lines() {
        if let Some((name, v)) = parse_line(line) {
            seen.insert(name.to_string(), v);
        }
    }
    let mut expected: Vec<&str> = VARIABLES.iter().map(|(_, v)| *v).collect();
    expected.sort_unstable();
    expected.dedup();
    let mut names: Vec<&str> = seen.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, expected, "`.env.example` and VARIABLES differ");
    for (name, v) in &seen {
        assert!(
            v.starts_with("dummy-") || v.contains("dummy"),
            "{name} in .env.example is not a dummy value: {v}"
        );
        assert!(!v.contains("ghp_") && !v.contains("sk-"), "{name}");
    }
    assert!(example.to_lowercase().contains("never commit"));
}
