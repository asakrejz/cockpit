//! Which language a review or a pull request text is written in: German or English.
//!
//! [`detect`] guesses from stopwords and umlauts. It says nothing when the
//! text is too short or too unclear. [`choose_review`] and [`choose_text`] apply the order of the
//! rules: explicit choice, detection, the repo's `pr_language`, English.

use cockpit_i18n::Lang;
use serde::{Deserialize, Serialize};

/// Fewest words (after removing code and links) a text needs before it is judged.
pub const MIN_WORDS: usize = 6;
/// Fewest stopword hits of both languages together.
pub const MIN_HITS: usize = 3;
/// Share of the hits the winner needs (0.0–1.0).
pub const MIN_SHARE: f64 = 0.7;
/// A pull request description with fewer letters than this is "short": the ticket is read too.
pub const SHORT_BODY: usize = 120;

/// Typical German words that are not English words as well (so `in`, `an`, `war`, `also` are left out).
const DE: &[&str] = &[
    "der", "die", "das", "den", "dem", "des", "und", "oder", "aber", "nicht", "ein", "eine",
    "einen", "einem", "einer", "ist", "sind", "wird", "werden", "wurde", "wurden", "hat", "haben",
    "kann", "können", "soll", "sollen", "muss", "müssen", "mit", "von", "für", "auf", "bei",
    "nach", "aus", "zum", "zur", "ich", "wir", "sie", "es", "dass", "wenn", "auch", "noch", "nur",
    "wie", "fehler", "damit", "dabei", "sich", "über", "unter", "zwischen", "wegen", "statt",
    "bitte", "neue", "neuen", "alle", "diese", "dieser", "dieses", "beim", "bis", "durch", "ohne",
    "jetzt", "nun",
];

/// Typical English words that are not German words as well.
const EN: &[&str] = &[
    "the", "and", "or", "but", "not", "is", "are", "was", "were", "be", "been", "has", "have",
    "had", "can", "should", "must", "will", "would", "with", "from", "for", "on", "at", "of", "to",
    "this", "that", "these", "those", "it", "its", "we", "you", "they", "when", "if", "also",
    "only", "how", "which", "while", "after", "before", "by", "into", "than", "then", "so", "as",
    "an", "a", "add", "adds", "fix", "fixes", "use", "uses", "now", "new", "all", "does", "do",
    "no", "instead",
];

/// Removes fenced code, inline code and links; they say nothing about the language.
fn prose(text: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut in_tick = false;
        for part in line.split('`') {
            if !in_tick {
                out.push_str(part);
            }
            in_tick = !in_tick;
            out.push(' ');
        }
        out.push('\n');
    }
    out
}

/// The language of `text`, or `None` when it is too short, has no prose (code only) or is unclear
/// (mixed). Pure and deterministic.
///
/// ```
/// use cockpit_core::language::detect;
/// use cockpit_i18n::Lang;
/// assert_eq!(detect("Der Filter wird nicht gespeichert, wenn die Seite neu geladen wird."), Some(Lang::De));
/// assert_eq!(detect("The filter is not saved when the page is reloaded by the user."), Some(Lang::En));
/// assert_eq!(detect("fix"), None);
/// ```
pub fn detect(text: &str) -> Option<Lang> {
    let prose = prose(text);
    let (mut de, mut en, mut words) = (0usize, 0usize, 0usize);
    for raw in prose.split(|c: char| !(c.is_alphabetic() || c == '\'')) {
        // Identifiers (`snake_case`, `camelCase`) are code, not words; `_` already split them.
        if raw.is_empty() || raw.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        let w = raw.to_lowercase();
        words += 1;
        if DE.contains(&w.as_str()) {
            de += 1;
        }
        if EN.contains(&w.as_str()) {
            en += 1;
        }
    }
    // Umlauts and ß hardly occur in English; a few of them help, they do not decide alone.
    de += prose
        .chars()
        .filter(|c| matches!(c, 'ä' | 'ö' | 'ü' | 'ß' | 'Ä' | 'Ö' | 'Ü'))
        .count()
        .min(3);
    let total = de + en;
    if words < MIN_WORDS || total < MIN_HITS {
        return None;
    }
    let (winner, hits) = if de > en {
        (Lang::De, de)
    } else {
        (Lang::En, en)
    };
    if de == en || (hits as f64) / (total as f64) < MIN_SHARE {
        return None;
    }
    Some(winner)
}

/// Why a language was chosen.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LangSource {
    /// The user picked it in the panel.
    Chosen,
    /// Detected from title and description of the pull request.
    PrText,
    /// Detected from the description of the linked ticket.
    Ticket,
    /// The `pr_language` of the repo (team default).
    Repo,
    /// Nothing else was known: English.
    Default,
}

/// A language and the reason for it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LangChoice {
    pub lang: Lang,
    pub source: LangSource,
}

fn letters(text: &str) -> usize {
    prose(text).chars().filter(|c| c.is_alphabetic()).count()
}

/// The language of a review: explicit choice, then detection from title and description (plus the
/// ticket description if the description is short), then the repo's language, then English.
pub fn choose_review(
    chosen: Option<Lang>,
    title: &str,
    description: &str,
    ticket_description: &str,
    repo: Option<Lang>,
) -> LangChoice {
    if let Some(lang) = chosen {
        return LangChoice {
            lang,
            source: LangSource::Chosen,
        };
    }
    let own = format!("{title}\n{description}");
    if let Some(lang) = detect(&own) {
        return LangChoice {
            lang,
            source: LangSource::PrText,
        };
    }
    if letters(description) < SHORT_BODY
        && let Some(lang) = detect(&format!("{own}\n{ticket_description}"))
    {
        let source = if detect(ticket_description).is_some() {
            LangSource::Ticket
        } else {
            LangSource::PrText
        };
        return LangChoice { lang, source };
    }
    fallback(repo)
}

/// The language of the text of a pull request the agent creates: detected from the ticket (title and
/// description), else the repo's language, else English.
pub fn choose_text(ticket_title: &str, ticket_description: &str, repo: Option<Lang>) -> LangChoice {
    match detect(&format!("{ticket_title}\n{ticket_description}")) {
        Some(lang) => LangChoice {
            lang,
            source: LangSource::Ticket,
        },
        None => fallback(repo),
    }
}

fn fallback(repo: Option<Lang>) -> LangChoice {
    match repo {
        Some(lang) => LangChoice {
            lang,
            source: LangSource::Repo,
        },
        None => LangChoice {
            lang: Lang::En,
            source: LangSource::Default,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_text() {
        let t = "Der Filter wird nach dem Neuladen nicht mehr gespeichert. Das Problem tritt auf, wenn die Seite über einen Link geöffnet wird.";
        assert_eq!(detect(t), Some(Lang::De));
    }

    #[test]
    fn english_text() {
        let t = "This fixes the filter that is not saved after a reload. The problem only happens when the page is opened from a link.";
        assert_eq!(detect(t), Some(Lang::En));
    }

    #[test]
    fn mixed_text_is_unclear() {
        let t = "Der Filter is not saved und the page wird nicht neu geladen when it is opened.";
        assert_eq!(detect(t), None);
    }

    #[test]
    fn code_only_has_no_language() {
        let t = "```rust\nfn main() { let der = die(); if das && und { return; } }\n```\n`fn der_die_das()`";
        assert_eq!(detect(t), None);
        assert_eq!(
            detect("fn load_user(id: u32) -> Result<User, Error> { db.get(id) }"),
            None
        );
    }

    #[test]
    fn code_inside_prose_is_ignored() {
        let t = "```\nthe and the and the and the and the\n```\nDer Fehler tritt auf, wenn die Liste leer ist und der Nutzer speichert.";
        assert_eq!(detect(t), Some(Lang::De));
    }

    #[test]
    fn very_short_texts_are_unclear() {
        assert_eq!(detect(""), None);
        assert_eq!(detect("Fix bug"), None);
        assert_eq!(detect("Fehler beheben für Übersicht"), None);
        assert_eq!(detect("Update the readme"), None);
    }

    #[test]
    fn deterministic() {
        let t = "Die Anmeldung schlägt fehl, wenn das Passwort ein Leerzeichen enthält.";
        assert_eq!(detect(t), detect(t));
    }

    #[test]
    fn order_of_the_rules() {
        let de = "Der Fehler tritt auf, wenn die Liste leer ist und der Nutzer speichert.";
        // 1. the explicit choice wins over everything
        let c = choose_review(Some(Lang::En), "Titel", de, "", Some(Lang::De));
        assert_eq!((c.lang, c.source), (Lang::En, LangSource::Chosen));
        // 2. detection from the pull request
        let c = choose_review(None, "Fix", de, "", Some(Lang::En));
        assert_eq!((c.lang, c.source), (Lang::De, LangSource::PrText));
        // 3. the repo
        let c = choose_review(None, "Fix", "", "", Some(Lang::De));
        assert_eq!((c.lang, c.source), (Lang::De, LangSource::Repo));
        // 4. English
        let c = choose_review(None, "Fix", "", "", None);
        assert_eq!((c.lang, c.source), (Lang::En, LangSource::Default));
    }

    #[test]
    fn short_body_reads_the_ticket() {
        let ticket =
            "Der Fehler tritt auf, wenn die Liste leer ist und der Nutzer die Seite speichert.";
        let c = choose_review(None, "Fix list", "Fixes it", ticket, Some(Lang::En));
        assert_eq!((c.lang, c.source), (Lang::De, LangSource::Ticket));
    }

    #[test]
    fn long_body_ignores_the_ticket() {
        let body = "xy ".repeat(70);
        let ticket =
            "Der Fehler tritt auf, wenn die Liste leer ist und der Nutzer die Seite speichert.";
        let c = choose_review(None, "Fix list", &body, ticket, Some(Lang::En));
        assert_eq!((c.lang, c.source), (Lang::En, LangSource::Repo));
    }

    #[test]
    fn agent_pull_request_text() {
        let c = choose_text(
            "Liste",
            "Die Liste ist leer, wenn der Nutzer keine Rechte hat und die Seite lädt.",
            Some(Lang::En),
        );
        assert_eq!((c.lang, c.source), (Lang::De, LangSource::Ticket));
        let c = choose_text("Liste", "", Some(Lang::De));
        assert_eq!((c.lang, c.source), (Lang::De, LangSource::Repo));
        let c = choose_text("Liste", "", None);
        assert_eq!((c.lang, c.source), (Lang::En, LangSource::Default));
    }
}
