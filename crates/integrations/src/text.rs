//! Small text tools for the mapping.

use cockpit_core::Person;

/// Initials from a name: two words → one letter each, one word → the first two.
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

/// Looks for a Jira key like `BAU-142` in the text (first hit).
pub fn ticket_key(text: &str) -> Option<String> {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        // Starts only at a word start.
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

/// Internal ID from parts: lowercase letters, everything else becomes `-`.
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
mod tests;
