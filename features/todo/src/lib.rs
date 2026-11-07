//! Eine kleine Todo-Liste.
//!
//! Zeigt: ein eigenes Datenmodell (`TodoList`) mit Unit-Tests,
//! Formulare, Listen mit `<For>` und abgeleitete Werte.

mod model;

pub use model::{Todo, TodoList};

use leptos::prelude::*;

#[component]
pub fn TodoApp(
    /// Einträge, mit denen die Liste startet.
    #[prop(optional)]
    initial: Vec<String>,
) -> impl IntoView {
    let todos = RwSignal::new(TodoList::from_texts(initial));
    let (draft, set_draft) = signal(String::new());

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        // `get_untracked`: wir lesen nur, ohne eine Abhängigkeit zu erzeugen.
        let text = draft.get_untracked();
        let mut added = false;
        todos.update(|list| added = list.add(&text).is_some());
        if added {
            set_draft.set(String::new());
        }
    };

    view! {
        <div class="todo">
            <form class="todo__form" on:submit=on_submit>
                <input
                    type="text"
                    placeholder="Was ist zu tun?"
                    prop:value=draft
                    on:input=move |ev| set_draft.set(event_target_value(&ev))
                />
                <button type="submit">"Hinzufügen"</button>
            </form>

            <ul class="todo__list">
                <For
                    each=move || todos.with(|list| list.items().to_vec())
                    // Der Key enthält `done`, damit ein Eintrag beim Abhaken neu gerendert wird.
                    key=|todo| (todo.id, todo.done)
                    children=move |todo| {
                        let id = todo.id;
                        view! {
                            <li class:todo__item=true class:todo__item--done=todo.done>
                                <label>
                                    <input
                                        type="checkbox"
                                        prop:checked=todo.done
                                        on:change=move |_| todos.update(|list| list.toggle(id))
                                    />
                                    <span>{todo.text}</span>
                                </label>
                                <button
                                    class="todo__remove"
                                    title="Entfernen"
                                    on:click=move |_| todos.update(|list| list.remove(id))
                                >
                                    "✕"
                                </button>
                            </li>
                        }
                    }
                />
            </ul>

            <p class="todo__summary">
                {move || {
                    let (open, total) = todos.with(|list| (list.remaining(), list.len()));
                    format!("{open} von {total} offen")
                }}
            </p>
        </div>
    }
}
