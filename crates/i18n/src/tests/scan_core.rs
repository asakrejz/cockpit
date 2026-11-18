//! Scan test: the core, the server and the integrations write no German text in their code.
//! Texts for people are codes with parameters (`msg!`); the catalogs hold the sentences.
//! Demo data (`demo/`) and test files are sample content and stay as they are.

use std::fs;

use super::{rust_files, workspace_root};
use super::scan::{literals, production_part};

const GERMAN_WORDS: &[&str] = &[
    "der", "die", "das", "nicht", "und", "wird", "kein", "keine", "für", "mit", "eine", "oder", "noch",
    "schon", "gibt", "kann", "von", "den", "dem", "sich", "wurde", "bitte", "wenn", "dann",
];

/// Literals that are German on purpose: old names that are still read as aliases, matching data
/// and the slug rules.
const ALLOWED_FILES: &[&str] = &[
    "crates/cockpit-core/src/text.rs",
    // Jira issue types and words in branch names are matching data.
    "crates/cockpit-core/src/branch.rs",
    // German option labels that only fill the German plain-text rendering of log entries and wire
    // fields; the interface maps these by id.
    "crates/cockpit-core/src/filters.rs",
    "crates/cockpit-core/src/model.rs",
    "crates/cockpit-core/src/model/status.rs",
    "crates/cockpit-core/src/workspace/inbox_filter.rs",
    // Simulated agent data (the demo agent): sample content like `demo/`.
    "crates/cockpit-core/src/review/checks.rs",
    "crates/cockpit-core/src/review/suggest.rs",
];

fn looks_german(text: &str) -> bool {
    if text.contains(['ä', 'ö', 'ü', 'Ä', 'Ö', 'Ü', 'ß']) {
        return true;
    }
    text.split(|c: char| !c.is_alphabetic())
        .any(|w| GERMAN_WORDS.contains(&w.to_lowercase().as_str()) && w.len() > 2)
}

fn is_sample_or_test(rel: &str) -> bool {
    rel.contains("/demo/") || rel.ends_with("/demo.rs") || rel.ends_with("tests.rs") || rel.contains("/tests/")
}

#[test]
fn no_german_text_literals_in_core_server_and_integrations() {
    let mut found = Vec::new();
    for dir in ["crates/cockpit-core/src", "apps/server/src", "crates/integrations/src"] {
        for file in rust_files(dir) {
            let rel = file.strip_prefix(workspace_root()).unwrap().to_string_lossy().to_string();
            if is_sample_or_test(&rel) || ALLOWED_FILES.contains(&rel.as_str()) {
                continue;
            }
            let source = fs::read_to_string(&file).unwrap();
            for (n, _, text) in literals(production_part(&source)) {
                if looks_german(&text) {
                    found.push(format!("{rel}:{n}: {text:?}"));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "German text belongs in crates/i18n/catalog (use msg!), not in the code:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_german_scan_finds_german_and_ignores_english() {
    assert!(looks_german("Das geht nicht"));
    assert!(looks_german("größer"));
    assert!(!looks_german("The branch does not exist"));
    assert!(!looks_german("claude/{ticket}-{slug}"));
}
