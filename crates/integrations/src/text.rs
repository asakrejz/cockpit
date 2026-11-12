//! Kleine Textwerkzeuge für die Abbildung.

use cockpit_core::Person;

/// Initialen aus einem Namen: zwei Wörter → je ein Buchstabe, ein Wort → die ersten zwei.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
        .filter(|w| !w.is_empty())
        .collect();
    let s: String = match words.as_slice() {
        [] => "?".into(),
        [w] => w.chars().take(2).collect(),
        [a, .., z] => a.chars().take(1).chain(z.chars().take(1)).collect(),
    };
    s.to_uppercase()
}

pub fn person(name: &str) -> Person {
    Person::new(name, &initials(name))
}

/// Sucht einen Jira-Schlüssel wie `BAU-142` im Text (erster Treffer).
pub fn ticket_key(text: &str) -> Option<String> {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        // Beginn nur am Wortanfang.
        let word_start = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if word_start && b[i].is_ascii_uppercase() {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_uppercase() || b[j].is_ascii_digit()) {
                j += 1;
            }
            if j - i >= 2 && j < b.len() && b[j] == b'-' {
                let k0 = j + 1;
                let mut k = k0;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                if k > k0 && (k == b.len() || !b[k].is_ascii_alphanumeric()) {
                    return Some(text[i..k].to_string());
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

/// Interne ID aus Teilen: Kleinbuchstaben, alles andere wird `-`.
pub fn id_from(parts: &[&str]) -> String {
    let raw = parts.join("-").to_lowercase();
    let mut out = String::new();
    for c in raw.chars() {
        let c = if c.is_alphanumeric() { c } else { '-' };
        if c != '-' || !out.ends_with('-') {
            out.push(c);
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
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
        assert_eq!(
            ticket_key("feature/BAU-142-avatar"),
            Some("BAU-142".into())
        );
        assert_eq!(ticket_key("BAU-7: Fix"), Some("BAU-7".into()));
        assert_eq!(ticket_key("kein Ticket hier"), None);
        assert_eq!(ticket_key("utf-8 und x-1"), None);
        assert_eq!(ticket_key("ABC-"), None);
    }

    #[test]
    fn id_is_lowercase_without_special_chars() {
        assert_eq!(id_from(&["gh", "Acme/Bau", "87"]), "gh-acme-bau-87");
    }
}
