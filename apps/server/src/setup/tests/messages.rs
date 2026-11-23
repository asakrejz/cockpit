//! The texts that reach the interface name fields of the assistant, never terminal commands.

use std::path::{Path, PathBuf};

use cockpit_core::setup::step_mentioned;
use cockpit_integrations::oauth::Service;

use crate::oauth::{secret_missing_msg, step_name};

/// Phrases of terminal commands that must not appear in a text for the interface.
const COMMANDS: &[&str] = &["token set", "token delete", "cargo run"];

#[test]
fn oauth_messages_name_the_step_and_the_field() {
    for service in [Service::GitHub, Service::Atlassian] {
        let text = secret_missing_msg(service).de();
        assert!(text.contains("Client-Secret"), "{text}");
        for command in COMMANDS {
            assert!(!text.contains(command), "{text}");
        }
        // The interface turns the named step into a link.
        let step = step_mentioned(&text).expect("names a step");
        assert_eq!(step.name(), step_name(service), "{text}");
    }
    assert_eq!(step_name(Service::GitHub), "GitHub");
    assert_eq!(step_name(Service::Atlassian), "Jira");
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// No text in the code of the interface and the server (string literals outside comments and
/// tests) tells the reader to run `cockpit-server token …`, `cargo run` and the like. Commands stay
/// in the docs for developers.
#[test]
fn no_text_for_the_interface_names_a_terminal_command() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["apps/cockpit/src", "apps/server/src", "crates"] {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(files.len() > 100, "found only {} files", files.len());
    let mut found = Vec::new();
    for file in files {
        let name = file.to_string_lossy().replace('\\', "/");
        // Tests and this very list may name the phrases; so do the usage text and the module
        // docs of the command line in `main.rs`.
        if name.contains("/tests") || name.ends_with("/main.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || !code.contains('"') {
                continue;
            }
            if COMMANDS.iter().any(|c| code.contains(c)) || code.contains("cockpit-server token") {
                found.push(format!("{name}:{}: {code}", n + 1));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}
