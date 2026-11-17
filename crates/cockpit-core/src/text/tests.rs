use super::slug;

#[test]
fn replaces_everything_but_letters_and_digits() {
    assert_eq!(slug("  Hallo,   Welt!! 2 ", 40), "hallo-welt-2");
    assert_eq!(slug("", 10), "");
    assert_eq!(slug("!!!", 10), "");
}

#[test]
fn transliterates_umlauts() {
    assert_eq!(slug("Größe für Übersicht", 40), "groesse-fuer-uebersicht");
}

#[test]
fn never_cuts_inside_a_word() {
    assert_eq!(
        slug("Toggle: Fokus-Ring fehlt im Dark Mode", 24),
        "toggle-fokus-ring-fehlt"
    );
    assert_eq!(
        slug("Sheet: Detents mittel und groß", 24),
        "sheet-detents-mittel-und"
    );
    assert_eq!(slug("abc def", 5), "abc");
    assert_eq!(
        slug("abcdefgh", 5),
        "",
        "ein einzelnes zu langes Wort passt nicht"
    );
}
