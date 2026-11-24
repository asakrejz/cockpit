//! `repo_name_valid` against the cases in `tests/fixtures/repo_name_cases.json`. The agent runner
//! repeats the rule in TypeScript (`repoNameValid` in `apps/agent/runner/src/job.ts`) and reads the
//! same file in `apps/agent/runner/test/job.test.ts`: a change to the rule has to change the file,
//! and then both sides have to agree again.

use cockpit_core::repo_name_valid;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    repo: String,
    expected: bool,
}

#[test]
fn repo_name_valid_follows_the_shared_cases() {
    let path = format!(
        "{}/tests/fixtures/repo_name_cases.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let cases: Vec<Case> =
        serde_json::from_str(&text).expect("repo_name_cases.json is a list of cases");
    assert!(cases.len() > 80, "the shared repo name cases went missing");
    assert!(
        cases.iter().any(|c| c.expected) && cases.iter().any(|c| !c.expected),
        "the shared repo name cases need valid and invalid names"
    );
    let wrong: Vec<String> = cases
        .iter()
        .filter(|c| repo_name_valid(&c.repo) != c.expected)
        .map(|c| format!("{:?}: expected {}", c.repo, c.expected))
        .collect();
    assert!(
        wrong.is_empty(),
        "repo_name_valid differs from the shared cases:\n{}",
        wrong.join("\n")
    );
}
