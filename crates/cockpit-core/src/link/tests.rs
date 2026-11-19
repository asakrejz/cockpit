use super::*;

#[test]
fn finds_keys_in_branch_names_and_titles() {
    assert_eq!(ticket_keys("feature/BAU-142-fokus-ring"), ["BAU-142"]);
    assert_eq!(ticket_keys("agent/bau-142-fokus"), ["BAU-142"]);
    assert_eq!(ticket_keys("BAU-142: Fokus-Ring im Dark Mode"), ["BAU-142"]);
    assert_eq!(ticket_keys("[BAU-142] Fokus"), ["BAU-142"]);
    assert_eq!(ticket_keys("Fix für (bau-142)."), ["BAU-142"]);
    assert_eq!(ticket_keys("feature/bau_142_x"), Vec::<String>::new());
    assert_eq!(ticket_keys("fix_BAU-142_x"), ["BAU-142"], "_ trennt");
}

#[test]
fn case_does_not_matter() {
    assert_eq!(ticket_keys("Bau-7"), ["BAU-7"]);
    assert_eq!(ticket_keys("bAu-7"), ["BAU-7"]);
}

#[test]
fn key_must_sit_on_word_boundaries() {
    // No false hit: BAU-1420 is a different ticket than BAU-142.
    assert_eq!(ticket_keys("bau-1420-foo"), ["BAU-1420"]);
    assert_eq!(find_ticket(&["bau-1420-foo"], ["BAU-142"]), None);
    assert_eq!(find_ticket(&["BAU-1420: Titel"], ["BAU-142"]), None);
    // No letter or digit may stick to the front either.
    assert_eq!(ticket_keys("xbau-142"), ["XBAU-142"]);
    assert_eq!(find_ticket(&["xbau-142"], ["BAU-142"]), None);
    assert_eq!(ticket_keys("v2bau-142"), Vec::<String>::new());
    // And nothing alphanumeric at the back.
    assert_eq!(find_ticket(&["BAU-142a"], ["BAU-142"]), None);
    assert_eq!(ticket_keys("BAU-142a"), Vec::<String>::new());
}

#[test]
fn no_false_positives_in_ordinary_text() {
    for text in [
        "",
        "-",
        "BAU",
        "BAU-",
        "-142",
        "BAU - 142",
        "BAU--142",
        "2026-11-11",
        "a-1",
        "Release v1-2",
        "feature/toggle-fokus",
    ] {
        assert_eq!(ticket_keys(text), Vec::<String>::new(), "{text:?}");
    }
}

#[test]
fn several_keys_keep_their_order_without_duplicates() {
    assert_eq!(
        ticket_keys("BAU-2 und bau-1, noch mal BAU-2"),
        ["BAU-2", "BAU-1"]
    );
}

#[test]
fn first_text_wins_and_known_spelling_is_returned() {
    let known = ["BAU-1", "BAU-2"];
    assert_eq!(
        find_ticket(&["feature/bau-2-x", "BAU-1: Titel"], known),
        Some("BAU-2")
    );
    // It skips unknown keys in the first text.
    assert_eq!(
        find_ticket(&["feature/XYZ-9-x", "BAU-1: Titel"], known),
        Some("BAU-1")
    );
    assert_eq!(find_ticket(&["nichts"], known), None);
    assert_eq!(find_ticket(&[], known), None);
}

#[test]
fn unicode_does_not_confuse_the_scanner() {
    assert_eq!(ticket_keys("Ärger-12 ü-3 BAU-5"), ["ÄRGER-12", "BAU-5"]);
}

#[test]
fn mentions_needs_the_whole_key_case_insensitive() {
    assert!(mentions(&["feature/app-123-foo"], "APP-123"));
    assert!(mentions(&["x", "APP-123: Titel"], "app-123"));
    assert!(mentions(&["Closes (APP-123)."], "APP-123"));
    assert!(!mentions(&["feature/app-1234-foo"], "APP-123"));
    assert!(!mentions(&["feature/xapp-123"], "APP-123"));
    assert!(!mentions(&["APP-12"], "APP-123"));
    assert!(!mentions(&[], "APP-123"));
}

#[test]
fn several_tickets_in_one_text() {
    let body = "Fixes APP-123 and app-124, see also APP-1234.";
    assert!(mentions(&[body], "APP-123"));
    assert!(mentions(&[body], "APP-124"));
    assert!(!mentions(&[body], "APP-12"));
}
