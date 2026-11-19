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

use cockpit_i18n::{Msg, msg};
use sha2::{Digest, Sha256};

/// Fixed name of the agent image (fully named so that Podman does not search the network for it).
pub const IMAGE: &str = "localhost/cockpit-agent:local";
/// Label on the image with the fingerprint of the sources it was built from.
pub const FINGERPRINT_LABEL: &str = "cockpit.fingerprint";
/// Raise this when the format of jobs or events between server and runner changes without any
/// file of the image changing (rare). It is part of the fingerprint.
pub const CONTRACT: u32 = 7;

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
pub fn fingerprint(dir: &Path) -> Result<String, Msg> {
    if !dir.is_dir() {
        return Err(msg!(
            ServerRunAgentDirMissing,
            dir = dir.display().to_string()
        ));
    }
    let mut files: Vec<(String, PathBuf)> = FILES
        .iter()
        .map(|f| ((*f).to_string(), dir.join(f)))
        .collect();
    let tree = dir.join(TREE);
    if !tree.is_dir() {
        return Err(msg!(
            ServerRunAgentTreeMissing,
            tree = TREE,
            dir = dir.display().to_string()
        ));
    }
    collect(&tree, TREE, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut h = Sha256::new();
    h.update(format!("cockpit-agent-image\ncontract={CONTRACT}\n"));
    for (name, path) in files {
        let bytes = std::fs::read(&path).map_err(|e| {
            msg!(
                ServerRunFileUnreadable,
                name = name.as_str(),
                error = e.to_string()
            )
        })?;
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

fn collect(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), Msg> {
    let unreadable =
        |e: std::io::Error| msg!(ServerRunDirUnreadable, dir = rel, error = e.to_string());
    let entries = std::fs::read_dir(dir).map_err(unreadable)?;
    for entry in entries {
        let entry = entry.map_err(unreadable)?;
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
mod tests;
