//! The language of the interface as a signal. Switching is instant: everything that reads the
//! language ([`lang`], [`tr`], [`t!`]) rebuilds when it changes.
//!
//! The texts are in `crates/i18n/catalog/<lang>/*.txt`; code uses the typed keys:
//!
//! ```ignore
//! t!(InboxTitle)                       // plain text
//! t!(InboxCount, n = items.len())      // parameters; plural keys use `n`
//! ```

use cockpit_i18n::Arg;
pub use cockpit_i18n::{Key, Lang};
use leptos::prelude::*;

/// The browser language: German for `de*`, otherwise English.
pub fn browser_lang() -> Lang {
    web_sys::window()
        .and_then(|w| w.navigator().language())
        .map(|tag| Lang::from_browser(&tag))
        .unwrap_or_default()
}

thread_local! {
    // One for the whole app (the browser has a single thread). Not tied to an owner, so it can be
    // read from event handlers and tests alike.
    static LANG: ArcRwSignal<Lang> = ArcRwSignal::new(browser_lang());
}

/// The current language. Reading inside a reactive scope subscribes to changes.
pub fn lang() -> Lang {
    LANG.with(|l| l.get())
}

/// Like [`lang`], without subscribing.
pub fn lang_untracked() -> Lang {
    LANG.with(|l| l.get_untracked())
}

/// Switches the language at once and updates `<html lang>`.
pub fn set_lang(lang: Lang) {
    LANG.with(|l| {
        if l.get_untracked() != lang {
            l.set(lang);
        }
    });
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("lang", lang.code());
    }
}

/// The text of `key` in the current language. Prefer the [`t!`] macro.
pub fn tr(key: Key, args: &[Arg]) -> String {
    cockpit_i18n::text(lang(), key, args)
}

/// How long ago `then_ms` is, in the current language ("5 minutes ago" / "vor 5 Minuten").
pub fn ago(then_ms: u64, now_ms: u64) -> String {
    cockpit_i18n::ago_ms(lang(), then_ms, now_ms)
}

/// A text of the interface: `t!(Key)` or `t!(Key, n = 3, name = x)`.
#[macro_export]
macro_rules! t {
    ($key:ident) => {
        $crate::tr($crate::Key::$key, &[])
    };
    ($key:ident, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::tr(
            $crate::Key::$key,
            &[$((stringify!($name), ($value).to_string())),+],
        )
    };
}

/// A text in a given language (for the language switcher, which shows both names).
pub fn name_of(lang: Lang) -> &'static str {
    lang.name()
}
