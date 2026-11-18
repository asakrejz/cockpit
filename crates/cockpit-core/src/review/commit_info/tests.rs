use super::*;

#[test]
fn the_date_is_shown_the_german_way() {
    assert_eq!(commit_date("2024-05-13T09:30:00Z"), "13.05.2024 09:30");
    assert_eq!(commit_date("2024-05-13"), "13.05.2024");
}

#[test]
fn something_else_stays_as_it_is() {
    assert_eq!(commit_date(""), "");
    assert_eq!(commit_date("gestern"), "gestern");
}

#[test]
fn the_sha_is_shortened() {
    assert_eq!(short_sha("aaaa1111aaaa1111"), "aaaa111");
    assert_eq!(short_sha("abc"), "abc");
}
