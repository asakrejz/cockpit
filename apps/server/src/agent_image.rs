//! Freshness of the agent image: a fixed name plus a fingerprint label.
//!
//! The image always has the name [`IMAGE`]. A build gives it the label [`FINGERPRINT_LABEL`] with
//! the SHA-256 over exactly the inputs that make the image. Whether an existing image is current
//! is decided by comparing that label with the fingerprint of the sources next to the server.
//!
//! Inputs, in this order of their relative paths (bytes, `/` as separator):
//!
//! * `Containerfile`
//! * `runner/package.json`, `runner/package-lock.json`
//! * `runner/src/**`: everything the Containerfile copies (`COPY runner/src`); `.DS_Store` and
//!   `node_modules` are skipped, symbolic links are not followed
//! * `clis/package.json`, `clis/package-lock.json` (the pinned agent CLIs)
//! * [`CONTRACT`]
//!
//! Not included: `runner/test/**` (tests are not copied into the image), `node_modules`, file
//! times and permissions. `\r\n` counts as `\n`, so the value does not depend on the platform.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Fixed name of the agent image (fully named so that Podman does not search the network for it).
pub const IMAGE: &str = "localhost/cockpit-agent:local";
/// Label on the image with the fingerprint of the sources it was built from.
pub const FINGERPRINT_LABEL: &str = "cockpit.fingerprint";
/// Raise this when the format of jobs or events between server and runner changes without any
/// file of the image changing (rare). It is part of the fingerprint.
pub const CONTRACT: u32 = 5;

/// Files that must exist, relative to the agent folder.
const FILES: [&str; 5] = [
    "Containerfile",
    "runner/package.json",
    "runner/package-lock.json",
    "clis/package.json",
    "clis/package-lock.json",
];
/// Folder whose whole tree is included.
const TREE: &str = "runner/src";

/// The fingerprint of the sources in `dir` (the `apps/agent` folder), lowercase hex.
/// `Err` with a plain-text reason if an input is missing or unreadable.
pub fn fingerprint(dir: &Path) -> Result<String, String> {
    if !dir.is_dir() {
        return Err(format!(
            "Der Ordner mit den Agent-Dateien fehlt: {}. Starte Cockpit aus dem Repo-Ordner oder setze COCKPIT_AGENT_DIR.",
            dir.display()
        ));
    }
    let mut files: Vec<(String, PathBuf)> = FILES
        .iter()
        .map(|f| ((*f).to_string(), dir.join(f)))
        .collect();
    let tree = dir.join(TREE);
    if !tree.is_dir() {
        return Err(format!("Der Ordner {TREE} fehlt in {}.", dir.display()));
    }
    collect(&tree, TREE, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut h = Sha256::new();
    h.update(format!("cockpit-agent-image\ncontract={CONTRACT}\n"));
    for (name, path) in files {
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("Die Datei {name} ist nicht lesbar ({e})."))?;
        let bytes = normalize(&bytes);
        h.update(name.as_bytes());
        h.update([0]);
        h.update(bytes.len().to_string().as_bytes());
        h.update([0]);
        h.update(&bytes);
        h.update(b"\n");
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn normalize(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn collect(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("Der Ordner {rel} ist nicht lesbar ({e})."))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Der Ordner {rel} ist nicht lesbar ({e})."))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "node_modules" || name == ".DS_Store" {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let rel = format!("{rel}/{name}");
        if kind.is_dir() {
            collect(&path, &rel, out)?;
        } else if kind.is_file() {
            out.push((rel, path));
        }
    }
    Ok(())
}

/// What `<cli> image inspect --format …` printed for the label, reduced to the value.
/// Docker prints an empty line, other versions `<no value>` or `<nil>` for a missing label.
pub fn label_value(output: &str) -> Option<&str> {
    let v = output.trim();
    (!v.is_empty() && v != "<no value>" && v != "<nil>" && v != "null").then_some(v)
}

/// The arguments of `<cli> image inspect` that print the fingerprint label.
pub fn inspect_args() -> [String; 4] {
    [
        "image".into(),
        "inspect".into(),
        "--format".into(),
        format!("{{{{ index .Config.Labels \"{FINGERPRINT_LABEL}\" }}}}"),
    ]
}

#[cfg(test)]
mod tests {
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
}
