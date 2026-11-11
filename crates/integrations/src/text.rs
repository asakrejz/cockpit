//! Kleine Textwerkzeuge für die Abbildung.

use cockpit_core::Person;

/// Initialen aus einem Namen: zwei Wörter → je ein Buchstabe, ein Wort → die ersten zwei.
pub fn initialen(name: &str) -> String {
    let woerter: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
        .filter(|w| !w.is_empty())
        .collect();
    let s: String = match woerter.as_slice() {
        [] => "?".into(),
        [w] => w.chars().take(2).collect(),
        [a, .., z] => a.chars().take(1).chain(z.chars().take(1)).collect(),
    };
    s.to_uppercase()
}

pub fn person(name: &str) -> Person {
    Person::new(name, &initialen(name))
}

/// Sucht einen Jira-Schlüssel wie `BAU-142` im Text (erster Treffer).
pub fn ticket_schluessel(text: &str) -> Option<String> {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        // Beginn nur am Wortanfang.
        let wortanfang = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if wortanfang && b[i].is_ascii_uppercase() {
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
pub fn id_aus(teile: &[&str]) -> String {
    let roh = teile.join("-").to_lowercase();
    let mut out = String::new();
    for c in roh.chars() {
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
    fn initialen_aus_namen() {
        assert_eq!(initialen("Mira Keller"), "MK");
        assert_eq!(initialen("octocat"), "OC");
        assert_eq!(initialen("anna-lena maier"), "AM");
        assert_eq!(initialen(""), "?");
    }

    #[test]
    fn findet_ticket_schluessel() {
        assert_eq!(
            ticket_schluessel("feature/BAU-142-avatar"),
            Some("BAU-142".into())
        );
        assert_eq!(ticket_schluessel("BAU-7: Fix"), Some("BAU-7".into()));
        assert_eq!(ticket_schluessel("kein Ticket hier"), None);
        assert_eq!(ticket_schluessel("utf-8 und x-1"), None);
        assert_eq!(ticket_schluessel("ABC-"), None);
    }

    #[test]
    fn id_ist_klein_und_ohne_sonderzeichen() {
        assert_eq!(id_aus(&["gh", "Acme/Bau", "87"]), "gh-acme-bau-87");
    }
}
