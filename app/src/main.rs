//! Die Playground-App: eine Navigation mit einem Reiter pro Story
//! und eine Bühne ("Stage"), auf der die gewählte Story gerendert wird.

mod stories;

use leptos::prelude::*;
use stories::{STORIES, Story};

fn main() {
    // Rust-Panics landen lesbar in der Browser-Konsole.
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

/// Liest die Story-ID aus der URL, z. B. `#counter` → `"counter"`.
fn story_from_hash() -> &'static Story {
    let hash = window().location().hash().unwrap_or_default();
    stories::find(hash.trim_start_matches('#')).unwrap_or(&STORIES[0])
}

#[component]
fn App() -> impl IntoView {
    let (current, set_current) = signal(story_from_hash());

    // Die URL ist die "Quelle der Wahrheit": Links ändern nur den Hash,
    // wir reagieren darauf. So funktionieren auch Zurück-Button und Bookmarks.
    let handle = window_event_listener(leptos::ev::hashchange, move |_| {
        set_current.set(story_from_hash());
    });
    on_cleanup(move || handle.remove());

    view! {
        <div class="layout">
            <nav class="sidebar">
                <h1 class="sidebar__title">"🦀 Playground"</h1>
                <ul>
                    {STORIES
                        .iter()
                        .map(|story| {
                            view! {
                                <li>
                                    <a
                                        href=format!("#{}", story.id)
                                        class:active=move || current.get().id == story.id
                                    >
                                        {story.title}
                                    </a>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
            </nav>

            <main class="stage">
                {move || {
                    let story = current.get();
                    view! {
                        <header class="stage__header">
                            <h2>{story.title}</h2>
                            <code class="stage__crate">{story.crate_name}</code>
                            <p>{story.description}</p>
                        </header>
                        <div class="stage__canvas">{(story.render)()}</div>
                    }
                }}
            </main>
        </div>
    }
}
