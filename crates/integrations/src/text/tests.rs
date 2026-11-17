use super::*;

#[test]
fn initials_from_names() {
    assert_eq!(initials("Mira Keller"), "MK");
    assert_eq!(initials("octocat"), "OC");
    assert_eq!(initials("anna-lena maier"), "AM");
    assert_eq!(initials(""), "?");
}

#[test]
fn finds_ticket_key() {
    assert_eq!(ticket_key("feature/BAU-142-avatar"), Some("BAU-142".into()));
    assert_eq!(ticket_key("BAU-7: Fix"), Some("BAU-7".into()));
    assert_eq!(ticket_key("kein Ticket hier"), None);
    assert_eq!(ticket_key("utf-8 und x-1"), None);
    assert_eq!(ticket_key("ABC-"), None);
}

#[test]
fn id_is_lowercase_without_special_chars() {
    assert_eq!(id_from(&["gh", "Acme/Bau", "87"]), "gh-acme-bau-87");
}
