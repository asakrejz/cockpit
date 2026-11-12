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
mod tests {
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
}
