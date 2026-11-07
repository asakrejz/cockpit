//! Die Playground-App: Navigation mit einem Eintrag pro Story und eine Bühne,
//! auf der die gewählte Story gerendert wird. Die Hülle benutzt selbst die
//! Design-Tokens und die UI-Komponenten.

mod stories;

use feature_ui::*;
use leptos::prelude::*;
use stories::{STORIES, Story};

fn main() {
    // Rust-Panics landen lesbar in der Browser-Konsole.
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

/// Liest die Story-ID aus der URL, z. B. `#button` → `"button"`.
fn story_from_hash() -> &'static Story {
    let hash = window().location().hash().unwrap_or_default();
    stories::find(hash.trim_start_matches('#')).unwrap_or(&STORIES[0])
}

/// Startet im Dunkelmodus, wenn das System ihn nutzt.
fn system_theme() -> &'static str {
    let dark = window()
        .match_media("(prefers-color-scheme: dark)")
        .ok()
        .flatten()
        .is_some_and(|m| m.matches());
    if dark { "dark" } else { "light" }
}

const THEMES: &[(&str, &str)] = &[("light", "Hell"), ("dark", "Dunkel")];

#[component]
fn App() -> impl IntoView {
    let (current, set_current) = signal(story_from_hash());

    // Die URL ist die "Quelle der Wahrheit": Links ändern nur den Hash, wir reagieren
    // darauf. So funktionieren auch Zurück-Button und Bookmarks.
    let handle = window_event_listener(leptos::ev::hashchange, move |_| {
        set_current.set(story_from_hash());
    });
    on_cleanup(move || handle.remove());

    // Erscheinungsbild – die Tokens lösen sich am `Root` neu auf.
    let theme_id = RwSignal::new(system_theme());
    let tint_id = RwSignal::new(Tint::Blue.id());
    let high_contrast = RwSignal::new(false);

    let theme = Signal::derive(move || {
        Theme::ALL
            .into_iter()
            .find(|t| t.id() == theme_id.get())
            .unwrap_or_default()
    });
    let tint = Signal::derive(move || {
        Tint::ALL
            .into_iter()
            .find(|t| t.id() == tint_id.get())
            .unwrap_or_default()
    });
    let contrast = Signal::derive(move || {
        if high_contrast.get() {
            Contrast::High
        } else {
            Contrast::Normal
        }
    });

    view! {
        <Root theme=theme tint=tint contrast=contrast class="pg">
            <aside class="pg__sidebar">
                <div class="pg__brand">
                    <Icon path=icons::BRAND size=22 />
                    <span class="t-headline">"Rust Playground"</span>
                </div>

                <nav aria-label="Stories" class="pg__nav">
                    {stories::groups()
                        .into_iter()
                        .map(|(group, items)| {
                            view! {
                                <p class="pg__group t-footnote t-semibold t-secondary">{group}</p>
                                <ul>
                                    {items
                                        .into_iter()
                                        .map(|story| {
                                            view! {
                                                <li>
                                                    <a
                                                        class="ui-navbar__link"
                                                        href=format!("#{}", story.id)
                                                        aria-current=move || {
                                                            (current.get().id == story.id).then_some("page")
                                                        }
                                                    >
                                                        {story.title}
                                                    </a>
                                                </li>
                                            }
                                        })
                                        .collect_view()}
                                </ul>
                            }
                        })
                        .collect_view()}
                </nav>

                <div class="pg__appearance">
                    <p class="t-footnote t-semibold t-secondary">"Erscheinungsbild"</p>
                    <Segmented options=THEMES value=theme_id aria_label="Theme" />
                    <div class="pg__swatches" role="group" aria-label="Akzentfarbe">
                        {Tint::ALL
                            .map(|t| {
                                view! {
                                    <button
                                        type="button"
                                        class="pg__swatch"
                                        style=format!("background: var(--sys-color-{})", t.id())
                                        aria-label=t.id()
                                        aria-pressed=move || (tint_id.get() == t.id()).to_string()
                                        on:click=move |_| tint_id.set(t.id())
                                    ></button>
                                }
                            })
                            .into_iter()
                            .collect_view()}
                    </div>
                    <label class="pg__contrast t-subheadline">
                        "Kontrast erhöhen"
                        <Toggle checked=high_contrast />
                    </label>
                </div>
            </aside>

            <main class="pg__stage">
                {move || {
                    let story = current.get();
                    view! {
                        <header class="pg__header">
                            <Text variant=TextStyle::Display tag=TextTag::H1>{story.title}</Text>
                            <Text variant=TextStyle::Title3 color=TextColor::Secondary>
                                {story.description}
                            </Text>
                            <Text variant=TextStyle::Footnote color=TextColor::Secondary mono=true>
                                {story.api}
                            </Text>
                        </header>
                        <div class="pg__canvas">{(story.render)()}</div>
                    }
                }}
            </main>
        </Root>
    }
}
