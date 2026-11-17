use super::*;

const P: &str = "feature/{ticket}-{slug}";

#[test]
fn default_pattern_is_valid_and_renders_like_before() {
    assert!(validate(DEFAULT_PATTERN).is_ok());
    assert_eq!(
        render(
            DEFAULT_PATTERN,
            "BAU-142",
            "Toggle: Fokus-Ring fehlt im Dark Mode",
            BranchType::Feature
        ),
        "claude/BAU-142-toggle-fokus-ring-fehlt"
    );
}

#[test]
fn renders_all_placeholders() {
    assert_eq!(
        render(
            "{type}/{ticket}-{slug}",
            "WEB-7",
            "Login kaputt",
            BranchType::Fix
        ),
        "fix/WEB-7-login-kaputt"
    );
    assert_eq!(
        render(P, "WEB-7", "!!!", BranchType::Feature),
        "feature/WEB-7"
    );
}

#[test]
fn an_invalid_pattern_renders_as_the_default() {
    assert_eq!(
        render("main", "A-1", "x", BranchType::Feature),
        "claude/A-1-x"
    );
}

#[test]
fn validation_rejects_bad_patterns() {
    for bad in [
        "",
        "   ",
        "main",
        "feature/fixed",
        "feature/{slug}",
        "{ticket}{slug}",
        "feature/{ticket}-{nope}",
        "feature/{ticket",
        "feature/{ticket}-}",
        "/feature/{ticket}",
        "feature/{ticket}/",
        "feature//{ticket}",
        "-x/{ticket}",
        "a..b/{ticket}",
        "feature/{ticket} {slug}",
        "feature/{ticket};rm",
        "x.lock/{ticket}.lock",
    ] {
        assert!(validate(bad).is_err(), "should be rejected: {bad:?}");
    }
    assert!(validate(&format!("a/{}{{ticket}}", "b".repeat(120))).is_err());
}

#[test]
fn validation_accepts_usual_patterns() {
    for ok in [
        "claude/{ticket}-{slug}",
        "feature/{ticket}-{slug}",
        "{type}/{ticket}-{slug}",
        "{ticket}-{slug}",
        "users/andre/{ticket}",
        "{ticket}",
    ] {
        assert!(validate(ok).is_ok(), "should be accepted: {ok}");
    }
}

#[test]
fn matching_follows_the_pattern() {
    assert!(matches(P, "feature/BAU-142-fokus-ring"));
    assert!(matches(P, "feature/BAU-142"));
    assert!(!matches(P, "claude/BAU-142-fokus-ring"));
    assert!(!matches(P, "feature/BAU-142-Fokus"));
    assert!(!matches(P, "feature/fokus-ring"));
    assert!(!matches(P, "feature/BAU-142-x/../main"));
    assert!(matches("{type}/{ticket}-{slug}", "chore/A-1-x"));
    assert!(!matches("{type}/{ticket}-{slug}", "bugfix/A-1-x"));
    assert!(!matches("main", "main"));
}

#[test]
fn everything_rendered_matches_and_checks() {
    for p in [
        DEFAULT_PATTERN,
        P,
        "{type}/{ticket}-{slug}",
        "{ticket}-{slug}",
        "{ticket}",
    ] {
        for t in [
            "Fix the 'login' bug!",
            "Größe",
            "",
            "a b c d e f g h i j k l m n o p",
        ] {
            let b = render(p, "WEB-12", t, BranchType::Chore);
            assert!(matches(p, &b), "{p} → {b}");
            assert!(check(p, "main", &b).is_ok(), "{p} → {b}");
        }
    }
}

#[test]
fn check_refuses_protected_names() {
    // Even a pattern that could produce them never gets them through.
    for name in [
        "main",
        "master",
        "HEAD",
        "develop",
        "Main",
        "trunk",
        "production",
    ] {
        assert!(check("{ticket}", "main", name).is_err(), "{name}");
        assert!(check_existing("{ticket}", "main", name).is_err(), "{name}");
    }
    assert!(check("{ticket}-{slug}", "release", "release").is_err());
}

#[test]
fn check_refuses_the_base_branch_and_odd_names() {
    assert!(check("{ticket}", "BAU-1", "BAU-1").is_err());
    for name in [
        "-x",
        "refs/heads/main",
        "a..b",
        "feature/",
        "feature//x",
        "a b",
        "x.lock",
        "",
    ] {
        assert!(check(P, "main", name).is_err(), "{name:?}");
    }
}

#[test]
fn check_refuses_a_name_outside_the_pattern() {
    let e = check(P, "main", "claude/BAU-1-x").unwrap_err();
    assert!(e.contains("passt nicht zum Muster"), "{e}");
}

#[test]
fn existing_branches_may_keep_the_default_pattern() {
    assert!(check_existing(P, "main", "claude/BAU-1-x").is_ok());
    assert!(check_existing(P, "main", "feature/BAU-1-x").is_ok());
    // Older pull requests of the agent keep working whatever follows `claude/`.
    assert!(check_existing(P, "main", "claude/x-1").is_ok());
    assert!(check_existing(P, "main", "claude/").is_err());
    assert!(check_existing(P, "main", "claude/../main").is_err());
    assert!(check_existing(P, "claude/x", "claude/x").is_err());
    assert!(check_existing(P, "main", "someone/else").is_err());
    assert!(is_agent_branch(P, "main", "feature/BAU-1-x"));
    assert!(!is_agent_branch(P, "main", "main"));
}

#[test]
fn types_follow_the_issue_type() {
    let none: &[String] = &[];
    assert_eq!(BranchType::of(Some("Bug"), none), BranchType::Fix);
    assert_eq!(BranchType::of(Some("Fehler"), none), BranchType::Fix);
    assert_eq!(BranchType::of(Some("Story"), none), BranchType::Feature);
    assert_eq!(BranchType::of(Some("Task"), none), BranchType::Chore);
    assert_eq!(BranchType::of(Some("Weird"), none), BranchType::Feature);
    assert_eq!(BranchType::of(None, none), BranchType::Feature);
    assert_eq!(
        BranchType::of(Some(" "), &["bug".to_string()]),
        BranchType::Fix
    );
    assert_eq!(BranchType::from_word("fix"), BranchType::Fix);
    assert_eq!(BranchType::from_word("zzz"), BranchType::Feature);
}
