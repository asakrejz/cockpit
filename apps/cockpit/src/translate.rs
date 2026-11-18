//! "Translate" for texts from Jira and GitHub (descriptions, comments).
//!
//! Only on a click, never by itself. The text goes to the model of the agent (through the local
//! server), so the first time a notice says so. "Show original" switches back. Translations are
//! kept in the app until the page is reloaded (the server keeps a cache as well). Without a key
//! for the model the button is disabled. The original is never changed.

use std::{cell::RefCell, collections::HashMap};

use cockpit_i18n::Lang;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::{server, store::use_store};

/// The notice was confirmed (stored only in this browser; losing it just shows the notice again).
const ACK: &str = "cockpit-translate-notice-ack";

thread_local! {
    /// `(text, language)` to translation.
    static CACHE: RefCell<HashMap<(String, Lang), String>> = RefCell::new(HashMap::new());
}

fn acknowledged() -> bool {
    window()
        .local_storage()
        .ok()
        .flatten()
        .is_some_and(|s| s.get_item(ACK).ok().flatten().is_some())
}

fn acknowledge() {
    if let Some(s) = window().local_storage().ok().flatten() {
        let _ = s.set_item(ACK, "1");
    }
}

/// Shows `render(None)` (the original) or `render(Some(translation))`, with the button below.
/// `text` is the plain text that is sent; empty text shows no button.
#[component]
pub fn Translatable(
    #[prop(into)] text: String,
    /// Draws the content: `None` for the original, `Some(text)` for the translation.
    render: Callback<Option<String>, AnyView>,
) -> impl IntoView {
    let store = use_store();
    let text = StoredValue::new(text);
    let translated: RwSignal<Option<String>> = RwSignal::new(None);
    let busy = RwSignal::new(false);
    let asking = RwSignal::new(false);
    let problem: RwSignal<Option<String>> = RwSignal::new(None);
    // The button works only with a key and a model for the agent.
    let available = Signal::derive(move || {
        store.health.with(|h| {
            h.as_ref()
                .is_some_and(|h| h.endpoint.key && h.endpoint.model.is_some())
        })
    });

    let run = move || {
        let to = lang_untracked();
        let original = text.get_value();
        if let Some(done) = CACHE.with(|c| c.borrow().get(&(original.clone(), to)).cloned()) {
            translated.set(Some(done));
            return;
        }
        busy.set(true);
        problem.set(None);
        spawn_local(async move {
            match server::post_translate(&original, to).await {
                Ok(done) => {
                    CACHE.with(|c| c.borrow_mut().insert((original, to), done.clone()));
                    translated.set(Some(done));
                }
                Err(e) => problem.set(Some(e.message())),
            }
            busy.set(false);
        });
    };
    let start = move |_| {
        if acknowledged() {
            run();
        } else {
            asking.set(true);
        }
    };
    let confirm = move |_| {
        acknowledge();
        asking.set(false);
        run();
    };

    view! {
        {move || render.run(translated.get())}
        <Show when=move || !text.get_value().trim().is_empty()>
            <div class="ck-translate">
                <Show
                    when=move || translated.with(Option::is_some)
                    fallback=move || {
                        view! {
                            <Button
                                variant=ButtonStyle::Borderless
                                size=ControlSize::Small
                                disabled=Signal::derive(move || busy.get() || !available.get())
                                on_click=start
                            >
                                {move || {
                                    if busy.get() {
                                        t!(TranslateBusy)
                                    } else {
                                        t!(TranslateButton, language = lang().name())
                                    }
                                }}
                            </Button>
                            <Show when=move || !available.get()>
                                <span class="t-footnote t-secondary">{t!(TranslateNoKey)}</span>
                            </Show>
                        }
                    }
                >
                    <span class="t-footnote t-secondary">{t!(TranslateDone)}</span>
                    <Button
                        variant=ButtonStyle::Borderless
                        size=ControlSize::Small
                        on_click=move |_| translated.set(None)
                    >
                        {t!(TranslateShowOriginal)}
                    </Button>
                </Show>
                <Show when=move || asking.get()>
                    <p class="ui-hint" role="status">{t!(TranslateNotice)}</p>
                    <HStack spacing="8px">
                        <Button
                            variant=ButtonStyle::GlassProminent
                            size=ControlSize::Small
                            on_click=confirm
                        >
                            {t!(TranslateConfirm)}
                        </Button>
                        <Button size=ControlSize::Small on_click=move |_| asking.set(false)>
                            {t!(TranslateCancel)}
                        </Button>
                    </HStack>
                </Show>
                {move || {
                    problem
                        .get()
                        .map(|m| view! { <p class="ui-hint ck-problem" role="alert">{m}</p> })
                }}
            </div>
        </Show>
    }
}
