//! A message as code and parameters: the wire and storage form of every text that the core and
//! the server produce for people (log entries, errors, hints, diagnoses). The language is chosen
//! only where it is shown ([`Msg::render`]), so a stored entry reads right in either language.
//!
//! The code is the id of a catalog key (`core.note.status_set`). A code that this build does not
//! know (a newer server) renders as the code itself, never as a crash.

use serde::{Deserialize, Serialize};

use crate::{Lang, catalog};

/// Value of a parameter: plain text, a nested message or a list of them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum ArgValue {
    Text(String),
    Msg(Msg),
    /// Items joined with `sep` (for example the reasons of a refusal).
    List {
        items: Vec<ArgValue>,
        sep: String,
    },
}

impl ArgValue {
    pub fn joined(items: impl IntoIterator<Item = impl Into<ArgValue>>, sep: &str) -> ArgValue {
        ArgValue::List {
            items: items.into_iter().map(Into::into).collect(),
            sep: sep.to_string(),
        }
    }

    fn render(&self, lang: Lang) -> String {
        match self {
            ArgValue::Text(t) => t.clone(),
            ArgValue::Msg(m) => m.render(lang),
            ArgValue::List { items, sep } => items
                .iter()
                .map(|i| i.render(lang))
                .collect::<Vec<_>>()
                .join(sep),
        }
    }
}

impl From<Msg> for ArgValue {
    fn from(m: Msg) -> Self {
        ArgValue::Msg(m)
    }
}
impl From<String> for ArgValue {
    fn from(s: String) -> Self {
        ArgValue::Text(s)
    }
}
impl From<&String> for ArgValue {
    fn from(s: &String) -> Self {
        ArgValue::Text(s.clone())
    }
}
impl From<&str> for ArgValue {
    fn from(s: &str) -> Self {
        ArgValue::Text(s.to_string())
    }
}
macro_rules! from_number {
    ($($t:ty),*) => {$(
        impl From<$t> for ArgValue {
            fn from(n: $t) -> Self {
                ArgValue::Text(n.to_string())
            }
        }
    )*};
}
from_number!(u8, u16, u32, u64, usize, i32, i64, isize, f64);

/// A text by code with its parameters.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Msg {
    pub code: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<(String, ArgValue)>,
}

impl Msg {
    pub fn new(key: crate::Key, args: Vec<(&str, ArgValue)>) -> Msg {
        Msg {
            code: key.id().to_string(),
            args: args.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }

    /// A text that comes from outside and has no code (a message of a service, a git error). It
    /// reads the same in every language.
    pub fn raw(text: impl Into<String>) -> Msg {
        Msg::new(
            crate::Key::CommonRaw,
            vec![("text", ArgValue::Text(text.into()))],
        )
    }

    /// The text in `lang`. Plural keys use the parameter `n`.
    pub fn render(&self, lang: Lang) -> String {
        let args: Vec<(String, String)> = self
            .args
            .iter()
            .map(|(k, v)| (k.clone(), v.render(lang)))
            .collect();
        catalog::text_by_id(lang, &self.code, &args)
    }

    /// The German text: what plain-text readers (old clients, searches) get next to the code.
    pub fn de(&self) -> String {
        self.render(Lang::De)
    }
}

/// `msg!(CoreNoteStatusSet, key = ticket, n = 3)` builds a [`Msg`]. Values can be text, numbers or
/// nested messages.
#[macro_export]
macro_rules! msg {
    ($key:ident) => {
        $crate::Msg::new($crate::Key::$key, ::std::vec::Vec::new())
    };
    ($key:ident, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::Msg::new(
            $crate::Key::$key,
            ::std::vec![$((stringify!($name), $crate::ArgValue::from($value))),+],
        )
    };
}
