//! `glob_match` against the cases in `tests/fixtures/glob_cases.json`. The agent runner has the same
//! rules in TypeScript (`globFits` in `apps/agent/runner/src/policy.ts`) and reads the same file in
//! `apps/agent/runner/test/policy.test.ts`: a change to the rules has to change the file, and then
//! both sides have to agree again.

use cockpit_core::rules::glob_match;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    pattern: String,
    path: String,
    expected: bool,
}

#[test]
fn glob_match_follows_the_shared_cases() {
    let path = format!(
        "{}/tests/fixtures/glob_cases.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let cases: Vec<Case> = serde_json::from_str(&text).expect("glob_cases.json is a list of cases");
    assert!(cases.len() > 100, "the shared glob cases went missing");
    let wrong: Vec<String> = cases
        .iter()
        .filter(|c| glob_match(&c.pattern, &c.path) != c.expected)
        .map(|c| {
            format!(
                "{:?} against {:?}: expected {}",
                c.pattern, c.path, c.expected
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "glob_match differs from the shared cases:\n{}",
        wrong.join("\n")
    );
}
