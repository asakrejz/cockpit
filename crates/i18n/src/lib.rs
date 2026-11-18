//! Texts of the interface in German and English.
//!
//! - The keys are a generated enum ([`Key`]), so a typo is a compile error.
//! - The texts live in `catalog/<lang>/<area>.txt` (one line `id = text`, `{name}` for parameters,
//!   `id.one` / `id.other` for plurals, `\n` for a line break).
//! - [`Lang`] is the language; the browser language is the default ([`Lang::from_browser`]).
//!
//! The crate knows nothing about Leptos. The reactive switch is in `feature-ui`.

use std::{collections::HashMap, sync::OnceLock};

use serde::{Deserialize, Serialize};

mod catalog;
mod msg;
mod time;

pub use catalog::{placeholders, plural_forms};
pub use msg::{ArgValue, Msg};
pub use time::{ago, ago_ms};

include!(concat!(env!("OUT_DIR"), "/keys.rs"));

/// Language of the interface.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    /// Used when the browser language is not German.
    #[default]
    En,
    De,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::De, Lang::En];

    /// `de`, `de-AT` … give German, everything else English.
    pub fn from_browser(tag: &str) -> Lang {
        if tag.trim().to_ascii_lowercase().starts_with("de") {
            Lang::De
        } else {
            Lang::En
        }
    }

    /// Code for `<html lang>` and the wire.
    pub const fn code(self) -> &'static str {
        match self {
            Lang::De => "de",
            Lang::En => "en",
        }
    }

    /// Name of the language in the language itself (for the switcher).
    pub const fn name(self) -> &'static str {
        match self {
            Lang::De => "Deutsch",
            Lang::En => "English",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }
}

/// Parameter of a text: `("n", "3".to_string())`.
pub type Arg = (&'static str, String);

/// The text of `key` in `lang`. Plural keys use the parameter `n`.
/// A missing text falls back to English and then to the id (a test makes sure that never happens).
pub fn text(lang: Lang, key: Key, args: &[Arg]) -> String {
    let args: Vec<(String, String)> = args
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    catalog::text_by_id(lang, key.id(), &args)
}

/// `text` without parameters.
pub fn plain(lang: Lang, key: Key) -> String {
    text(lang, key, &[])
}

pub(crate) fn tables() -> &'static HashMap<Lang, HashMap<String, String>> {
    static TABLES: OnceLock<HashMap<Lang, HashMap<String, String>>> = OnceLock::new();
    TABLES.get_or_init(|| {
        [(Lang::En, EN_FILES), (Lang::De, DE_FILES)]
            .into_iter()
            .map(|(lang, files)| (lang, catalog::parse_all(files)))
            .collect()
    })
}

#[cfg(test)]
mod tests;
