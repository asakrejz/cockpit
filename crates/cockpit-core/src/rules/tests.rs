use super::*;
use cockpit_i18n::Lang;

fn files(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn stars_stay_inside_a_folder_name() {
    assert!(glob_match("styles/*.css", "styles/tokens.css"));
    assert!(!glob_match("styles/*.css", "styles/a/tokens.css"));
    assert!(!glob_match("styles/*.css", "other/styles/tokens.css"));
    assert!(glob_match("src/lib.rs", "src/lib.rs"));
    assert!(!glob_match("src/lib.rs", "src/lib.rs.bak"));
    assert!(glob_match("a*c", "abbbc"));
    assert!(glob_match("a*c", "ac"));
    assert!(!glob_match("a*c", "abbbd"));
    assert!(glob_match("*", "datei"));
    assert!(glob_match("fil?.rs", "file.rs"));
    assert!(!glob_match("fil?.rs", "fil.rs"));
}

#[test]
fn double_star_spans_folders() {
    assert!(glob_match("**/*.rs", "a/b/c.rs"));
    assert!(glob_match("**/*.rs", "c.rs"));
    assert!(glob_match("src/**", "src/a/b.rs"));
    assert!(glob_match("src/**", "src"));
    assert!(!glob_match("src/**", "other/src/a.rs"));
    assert!(glob_match("src/**/mod.rs", "src/mod.rs"));
    assert!(glob_match("src/**/mod.rs", "src/a/b/mod.rs"));
    assert!(!glob_match("src/**/mod.rs", "src/a/b/lib.rs"));
    assert!(glob_match("**", "egal/was"));
}

#[test]
fn patterns_without_a_slash_match_in_every_folder() {
    assert!(glob_match("*.lock", "Cargo.lock"));
    assert!(glob_match("*.lock", "a/b/yarn.lock"));
    assert!(!glob_match("*.lock", "a/lock.txt"));
    assert!(glob_match(".env", "server/.env"));
    assert!(glob_match(".env*", "x/.env.local"));
}

#[test]
fn a_trailing_slash_means_the_whole_folder() {
    assert!(glob_match("docs/", "docs/a/b.md"));
    assert!(glob_match(".github/", ".github/workflows/ci.yml"));
    assert!(!glob_match("docs/", "other/docs.md"));
}

#[test]
fn leading_dot_slash_and_slash_are_ignored() {
    assert!(glob_match("./src/*.rs", "src/a.rs"));
    assert!(glob_match("/src/*.rs", "./src/a.rs"));
    assert!(glob_match("src/*.rs", "/src/a.rs"));
}

#[test]
fn empty_patterns_match_nothing() {
    assert!(!glob_match("", "a"));
    assert!(!glob_match("   ", "a"));
}

#[test]
fn no_rules_allow_everything() {
    let rules = RepoRules::default();
    assert!(rules.is_empty());
    assert!(
        rules
            .violations(&files(&["a.rs", ".github/x.yml"]))
            .is_empty()
    );
    assert!(rules.lines_msg().is_empty());
}

#[test]
fn forbidden_paths_are_reported_with_their_pattern() {
    let rules = RepoRules {
        forbidden_paths: vec![".github/**".into(), "*.lock".into()],
        ..RepoRules::default()
    };
    let v = rules.violations(&files(&["src/a.rs", ".github/ci.yml", "Cargo.lock"]));
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].file, ".github/ci.yml");
    assert_eq!(v[0].kind, ViolationKind::Forbidden);
    assert_eq!(v[0].pattern, ".github/**");
    assert_eq!(v[1].pattern, "*.lock");
    assert_eq!(
        v[0].reason_msg().render(Lang::De),
        ".github/ci.yml ist verboten (Regel „.github/**“)"
    );
}

#[test]
fn allowed_paths_exclude_everything_else() {
    let rules = RepoRules {
        allowed_paths: vec!["src/**".into(), "tests/**".into()],
        ..RepoRules::default()
    };
    let v = rules.violations(&files(&["src/a.rs", "tests/t.rs", "README.md"]));
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].file, "README.md");
    assert_eq!(v[0].kind, ViolationKind::NotAllowed);
    assert_eq!(
        v[0].reason_msg().render(Lang::De),
        "README.md liegt außerhalb der erlaubten Pfade"
    );
}

#[test]
fn forbidden_beats_allowed() {
    let rules = RepoRules {
        allowed_paths: vec!["src/**".into()],
        forbidden_paths: vec!["src/secret/**".into()],
        ..RepoRules::default()
    };
    let v = rules.violations(&files(&["src/ok.rs", "src/secret/key.rs"]));
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].kind, ViolationKind::Forbidden);
}

#[test]
fn cleaning_trims_and_removes_duplicates_and_blanks() {
    let rules = RepoRules {
        allowed_paths: vec![" src/** ".into(), "".into(), "src/**".into()],
        forbidden_paths: vec!["  ".into()],
        required_tests: vec!["cargo test ".into(), "cargo test".into()],
        instructions: "  Kurz halten.  ".into(),
        plan_required: true,
    }
    .cleaned();
    assert_eq!(rules.allowed_paths, ["src/**"]);
    assert!(rules.forbidden_paths.is_empty());
    assert_eq!(rules.required_tests, ["cargo test"]);
    assert_eq!(rules.instructions, "Kurz halten.");
}

#[test]
fn lines_name_every_active_rule() {
    let rules = RepoRules {
        allowed_paths: vec!["src/**".into()],
        forbidden_paths: vec!["*.lock".into()],
        required_tests: vec!["cargo test".into(), "cargo clippy".into()],
        instructions: "Kleine Commits.".into(),
        plan_required: true,
    };
    let lines: Vec<String> = rules
        .lines_msg()
        .iter()
        .map(|m| m.render(Lang::De))
        .collect();
    assert_eq!(
        lines,
        [
            "Erlaubt nur: src/**",
            "Verboten: *.lock",
            "Pflicht-Tests vor dem Pull Request: cargo test, cargo clippy",
            "Plan-Freigabe ist Pflicht",
            "Anweisung: Kleine Commits.",
        ]
    );
}

#[test]
fn old_data_without_fields_loads() {
    let rules: RepoRules = serde_json::from_str("{}").unwrap();
    assert!(rules.is_empty());
    let rules: RepoRules = serde_json::from_str(r#"{"plan_required": true}"#).unwrap();
    assert!(rules.plan_required);
}
