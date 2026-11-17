use super::*;

#[test]
fn feature_prefix_with_a_ticket() {
    let s = suggest("Branches are named feature/PROJ-123-short-title.").unwrap();
    assert_eq!(s.pattern, "feature/{ticket}-{slug}");
}

#[test]
fn feature_and_fix_become_the_type_placeholder() {
    let s = suggest("Use feature/… for new work and fix/… for bugs.").unwrap();
    assert_eq!(s.pattern, "{type}/{ticket}-{slug}");
}

#[test]
fn bugfix_is_not_mistaken_for_fix() {
    let s = suggest("Use feature/x and bugfix/y").unwrap();
    assert_eq!(s.pattern, "feature/{ticket}-{slug}");
    assert!(s.reason.contains("bugfix/"));
}

#[test]
fn only_a_ticket_key_needs_the_word_branch() {
    let s = suggest("Your branch should start with the ticket, e.g. PROJ-123-login.").unwrap();
    assert_eq!(s.pattern, "{ticket}-{slug}");
    assert!(suggest("Tickets look like PROJ-123.").is_none());
}

#[test]
fn nothing_found_gives_none() {
    assert!(suggest("Be kind. Run the tests.").is_none());
    assert!(suggest("").is_none());
}

#[test]
fn every_suggestion_is_a_valid_pattern() {
    for text in [
        "feature/PROJ-1",
        "feature/ and fix/",
        "hotfix/ only",
        "branch PROJ-12",
    ] {
        let s = suggest(text).unwrap();
        assert!(crate::branch::validate(&s.pattern).is_ok(), "{}", s.pattern);
    }
}
