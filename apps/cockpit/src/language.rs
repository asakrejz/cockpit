//! The language of the interface: which one applies and the switcher.
//!
//! The choice is stored in the workspace (`Action::SetLanguage`). Until it is chosen, the browser
//! language applies. The switch is instant: the signal in `feature-ui` changes first, the server
//! keeps the choice.

use cockpit_core::Action;
use feature_ui::*;
use leptos::prelude::*;

use crate::store::use_store;

const OPTIONS: &[(&str, &str)] = &[
    (Lang::De.code(), Lang::De.name()),
    (Lang::En.code(), Lang::En.name()),
];

/// Keeps the language signal in line with the workspace: the saved choice wins over the browser.
/// When nothing is chosen yet, the browser language is saved once, so the server (and with it the
/// agents) know which language to write in.
pub fn follow_workspace() {
    let store = use_store();
    let saved = StoredValue::new(false);
    Effect::new(move |_| {
        let chosen = store.ws.with(|w| w.language);
        let language = chosen.unwrap_or_else(browser_lang);
        set_lang(language);
        if chosen.is_none() && store.loaded.get() && !saved.get_value() {
            saved.set_value(true);
            let _ = store.dispatch(Action::SetLanguage { language });
        }
    });
}

/// Two buttons, "Deutsch" and "English". Place it where the language can be chosen.
#[component]
pub fn LanguageSwitch(#[prop(optional)] stretch: bool) -> impl IntoView {
    let store = use_store();
    let value = Signal::derive(move || lang().code());
    let on_change = Callback::new(move |code: &'static str| {
        let Some(language) = Lang::from_code(code) else {
            return;
        };
        // At once; the server confirms with the next state.
        set_lang(language);
        let _ = store.dispatch(Action::SetLanguage { language });
    });
    view! {
        <SegmentedPicker
            options=OPTIONS
            value=value
            on_change=on_change
            aria_label=t!(LanguageLabel)
            stretch=stretch
        />
    }
}
