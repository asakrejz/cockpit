//! Textfunktionen.

/// Macht aus einem Titel einen Namensteil für Branches: Kleinbuchstaben, Ziffern
/// und einzelne Bindestriche, höchstens `max` Zeichen, nie mitten im Wort gekappt.
///
/// ```
/// use cockpit_core::text::slug;
/// assert_eq!(slug("Toggle: Fokus-Ring fehlt im Dark Mode", 24), "toggle-fokus-ring-fehlt");
/// ```
pub fn slug(title: &str, max: usize) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_alphanumeric() => out.push(c),
            // Alles andere trennt Wörter – aber nie doppelt und nie am Anfang.
            _ if !out.is_empty() && !out.ends_with('-') => out.push('-'),
            _ => {}
        }
    }
    // `out` ist reines ASCII, Byte-Positionen sind also Zeichen-Positionen.
    if out.len() > max {
        let cut_inside_word = out.as_bytes()[max] != b'-';
        out.truncate(max);
        if cut_inside_word {
            let last_word = out.rfind('-').unwrap_or(0);
            out.truncate(last_word);
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
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
}
