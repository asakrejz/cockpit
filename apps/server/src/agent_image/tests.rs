use super::*;

fn layout(d: &Path) {
    let w = |p: &str, c: &str| {
        let f = d.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    };
    w("Containerfile", "FROM scratch\n");
    w("runner/package.json", "{}\n");
    w("runner/package-lock.json", "{}\n");
    w("runner/src/main.ts", "a\n");
    w("runner/src/adapter/x.ts", "b\n");
    w("runner/test/main.test.ts", "t\n");
    w("clis/package.json", "{}\n");
    w("clis/package-lock.json", "{}\n");
}

fn fp(d: &Path) -> String {
    fingerprint(d).unwrap()
}

#[test]
fn stable_and_hex() {
    let t = tempfile::tempdir().unwrap();
    layout(t.path());
    let a = fp(t.path());
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(a, fp(t.path()));
}

#[test]
fn runner_change_changes_but_test_change_does_not() {
    let t = tempfile::tempdir().unwrap();
    layout(t.path());
    let a = fp(t.path());
    std::fs::write(t.path().join("runner/test/main.test.ts"), "other\n").unwrap();
    std::fs::write(t.path().join("runner/test/new.test.ts"), "new\n").unwrap();
    assert_eq!(a, fp(t.path()));
    std::fs::write(t.path().join("runner/src/adapter/x.ts"), "c\n").unwrap();
    assert_ne!(a, fp(t.path()));
}

#[test]
fn every_input_counts() {
    let t = tempfile::tempdir().unwrap();
    layout(t.path());
    let a = fp(t.path());
    for f in FILES {
        let p = t.path().join(f);
        let old = std::fs::read(&p).unwrap();
        std::fs::write(&p, b"changed\n").unwrap();
        assert_ne!(a, fp(t.path()), "{f}");
        std::fs::write(&p, old).unwrap();
    }
    assert_eq!(a, fp(t.path()));
}

#[test]
fn file_names_count_not_only_contents() {
    let t = tempfile::tempdir().unwrap();
    layout(t.path());
    let a = fp(t.path());
    std::fs::rename(
        t.path().join("runner/src/main.ts"),
        t.path().join("runner/src/start.ts"),
    )
    .unwrap();
    assert_ne!(a, fp(t.path()));
    // Same contents moved between two files: names and order both matter.
    std::fs::rename(
        t.path().join("runner/src/start.ts"),
        t.path().join("runner/src/main.ts"),
    )
    .unwrap();
    assert_eq!(a, fp(t.path()));
    std::fs::write(t.path().join("runner/src/main.ts"), "b\n").unwrap();
    std::fs::write(t.path().join("runner/src/adapter/x.ts"), "a\n").unwrap();
    assert_ne!(a, fp(t.path()));
}

#[test]
fn line_endings_do_not_matter() {
    let t = tempfile::tempdir().unwrap();
    layout(t.path());
    let a = fp(t.path());
    std::fs::write(t.path().join("Containerfile"), "FROM scratch\r\n").unwrap();
    std::fs::write(t.path().join("runner/src/main.ts"), "a\r\n").unwrap();
    assert_eq!(a, fp(t.path()));
}

#[test]
fn node_modules_and_ds_store_are_ignored() {
    let t = tempfile::tempdir().unwrap();
    layout(t.path());
    let a = fp(t.path());
    std::fs::create_dir_all(t.path().join("runner/src/node_modules/x")).unwrap();
    std::fs::write(t.path().join("runner/src/node_modules/x/i.js"), "z").unwrap();
    std::fs::create_dir_all(t.path().join("runner/node_modules")).unwrap();
    std::fs::write(t.path().join("runner/node_modules/i.js"), "z").unwrap();
    std::fs::write(t.path().join("runner/src/.DS_Store"), "z").unwrap();
    assert_eq!(a, fp(t.path()));
}

#[test]
fn missing_folder_or_input_is_an_error() {
    let t = tempfile::tempdir().unwrap();
    let e = fingerprint(&t.path().join("nope")).unwrap_err();
    assert!(e.contains("fehlt"), "{e}");
    layout(t.path());
    std::fs::remove_file(t.path().join("clis/package-lock.json")).unwrap();
    let e = fingerprint(t.path()).unwrap_err();
    assert!(e.contains("clis/package-lock.json"), "{e}");
}

#[test]
fn label_values() {
    assert_eq!(label_value("abc\n"), Some("abc"));
    assert_eq!(label_value("\n"), None);
    assert_eq!(label_value("<no value>\n"), None);
    assert_eq!(label_value("<nil>"), None);
}
