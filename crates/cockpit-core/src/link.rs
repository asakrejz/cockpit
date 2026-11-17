//! Link ticket and pull request: find the ticket key in branch names and titles.
//!
//! Everything here is pure functions without state. A key consists of letters,
//! a hyphen and digits (`BAU-142`). It only counts when it stands as a whole word:
//! `BAU-1420` does not contain `BAU-142`, and `XBAU-142` does not either. Upper and
//! lower case do not matter (`bau-142-toggle`), separators like `/`, `_`, `:`, `.` or spaces
//! delimit the word.

/// All keys in a text, uppercased and in order of appearance.
///
/// ```
/// use cockpit_core::link::ticket_keys;
/// assert_eq!(ticket_keys("feature/bau-142-fokus"), ["BAU-142"]);
/// ```
pub fn ticket_keys(text: &str) -> Vec<String> {
    // The text breaks into words of letters and digits and the separators between them.
    let mut words: Vec<(&str, &str)> = Vec::new(); // (word, separator after it)
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                let end = text[i..]
                    .find(char::is_alphanumeric)
                    .map_or(text.len(), |n| i + n);
                words.push((&text[s..i], &text[i..end]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        words.push((&text[s..], ""));
    }

    let letters = |w: &str| !w.is_empty() && w.chars().all(char::is_alphabetic);
    let digits = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit());
    let mut keys = Vec::new();
    for pair in words.windows(2) {
        let ((project, sep), (number, _)) = (pair[0], pair[1]);
        if sep == "-" && letters(project) && project.chars().count() >= 2 && digits(number) {
            let key = format!("{project}-{number}").to_uppercase();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

/// The first known key that appears in one of the texts. The texts count in their
/// order (first branch, then title). Returns the key the way the
/// caller knows.
pub fn find_ticket<'a>(
    texts: &[&str],
    known: impl IntoIterator<Item = &'a str> + Clone,
) -> Option<&'a str> {
    texts.iter().find_map(|text| {
        ticket_keys(text).into_iter().find_map(|found| {
            known
                .clone()
                .into_iter()
                .find(|key| key.eq_ignore_ascii_case(&found))
        })
    })
}

#[cfg(test)]
mod tests;
