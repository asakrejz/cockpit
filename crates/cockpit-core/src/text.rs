//! Text functions.

/// Turns a title into a name part for branches: lowercase letters, digits
/// and single hyphens, at most `max` characters, never cut in the middle of a word.
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
            // Everything else separates words – but never twice and never at the start.
            _ if !out.is_empty() && !out.ends_with('-') => out.push('-'),
            _ => {}
        }
    }
    // `out` is pure ASCII, so byte positions are character positions.
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
mod tests;
