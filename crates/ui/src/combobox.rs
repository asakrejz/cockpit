use leptos::ev::{FocusEvent, KeyboardEvent};
use leptos::prelude::*;

use crate::{Avatar, next_id};

/// One suggestion of a [`Combobox`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboOption {
    /// What `on_select` gets back.
    pub id: String,
    /// The text of the option (and what the field shows once it is chosen).
    pub label: String,
    /// With initials the option gets a small avatar.
    pub initials: Option<String>,
    /// A thin line below this option separates „quick choices“ from the search results.
    pub divider: bool,
}

impl ComboOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            initials: None,
            divider: false,
        }
    }

    pub fn initials(mut self, initials: impl Into<String>) -> Self {
        self.initials = Some(initials.into());
        self
    }

    pub fn divider(mut self) -> Self {
        self.divider = true;
        self
    }
}

/// Text field with an inline list of suggestions (ARIA combobox with a listbox).
///
/// The caller decides *what* is suggested: it reads `query` and passes the matching `options`
/// (ranked, cut to a sensible number, quick choices on top). The component takes care of the
/// rest: `role="combobox"`, `aria-expanded`, `aria-activedescendant`, arrow keys, Enter,
/// Escape, mouse and touch.
///
/// While the field is not being edited it shows `value` (e.g. the current choice); typing
/// replaces it by the query. Leaving the field without choosing keeps `value`.
#[component]
pub fn Combobox(
    #[prop(into)] label: String,
    /// The label stays for screen readers only (the surroundings already name the field).
    #[prop(optional)]
    label_hidden: bool,
    /// The current choice, shown while the user is not typing.
    #[prop(into)]
    value: Signal<String>,
    /// What the user types. Owned by the caller, who searches with it.
    query: RwSignal<String>,
    #[prop(into)] options: Signal<Vec<ComboOption>>,
    #[prop(into)] on_select: Callback<String>,
    #[prop(optional, into)] placeholder: Option<String>,
    /// Shown instead of the list while `options` is empty and the user has typed something.
    #[prop(optional, into)]
    empty_text: Option<String>,
    /// Shows a quiet „searching“ hint at the end of the list (e.g. while a server answers).
    #[prop(into, default = false.into())]
    busy: Signal<bool>,
    #[prop(optional, into)] busy_text: Option<String>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
) -> impl IntoView {
    let id = next_id("combo");
    let list_id = format!("{id}-list");
    let option_id = {
        let id = id.clone();
        move |i: usize| format!("{id}-opt-{i}")
    };
    let open = RwSignal::new(false);
    let editing = RwSignal::new(false);
    let active = RwSignal::new(None::<usize>);
    // Set by the focus; the mouse-up that ends the same click must not undo the selection.
    let just_focused = RwSignal::new(false);

    let count = move || options.with(Vec::len);
    let close = move || {
        open.set(false);
        editing.set(false);
        active.set(None);
        query.set(String::new());
    };
    let choose = move |i: usize| {
        if let Some(o) = options.with(|o| o.get(i).cloned()) {
            // First tell the caller (its options still match the query), then reset the field.
            on_select.run(o.id);
            close();
        }
    };
    // New suggestions: nothing is highlighted any more, unless it was one that still exists.
    Effect::new(move |_| {
        let n = count();
        active.update(|a| {
            if a.is_some_and(|i| i >= n) {
                *a = None;
            }
        });
    });

    let on_key = move |ev: KeyboardEvent| match ev.key().as_str() {
        "ArrowDown" => {
            ev.prevent_default();
            let n = count();
            if !open.get_untracked() {
                open.set(true);
            }
            if n > 0 {
                active.update(|a| *a = Some(a.map_or(0, |i| (i + 1) % n)));
            }
        }
        "ArrowUp" => {
            ev.prevent_default();
            let n = count();
            if !open.get_untracked() {
                open.set(true);
            }
            if n > 0 {
                active.update(|a| *a = Some(a.map_or(n - 1, |i| (i + n - 1) % n)));
            }
        }
        "Enter" => {
            if open.get_untracked()
                && let Some(i) = active.get_untracked()
            {
                ev.prevent_default();
                choose(i);
            }
        }
        "Escape" if open.get_untracked() || editing.get_untracked() => {
            ev.prevent_default();
            ev.stop_propagation();
            close();
        }
        _ => {}
    };
    let on_blur = move |_: FocusEvent| close();

    let shown = move || {
        if editing.get() {
            query.get()
        } else {
            value.get()
        }
    };
    let empty_text = StoredValue::new(empty_text);
    let busy_text = StoredValue::new(busy_text);
    let option_id_view = option_id.clone();

    view! {
        <div class="ui-field ui-combo">
            <label class="ui-field__label" class:ui-sr-only=label_hidden for=id.clone()>{label}</label>
            <input
                class="ui-field__input"
                id=id.clone()
                type="text"
                role="combobox"
                autocomplete="off"
                spellcheck="false"
                aria-autocomplete="list"
                aria-haspopup="listbox"
                aria-controls=list_id.clone()
                aria-expanded=move || open.get().to_string()
                aria-activedescendant=move || active.get().map(option_id.clone())
                placeholder=placeholder
                disabled=move || disabled.get()
                prop:value=shown
                on:focus=move |ev| {
                    // The current choice is selected, so typing replaces it.
                    open.set(true);
                    just_focused.set(true);
                    event_target::<web_sys::HtmlInputElement>(&ev).select();
                }
                on:input=move |ev| {
                    editing.set(true);
                    open.set(true);
                    active.set(None);
                    query.set(event_target_value(&ev));
                }
                on:mouseup=move |ev| {
                    if just_focused.get_untracked() {
                        ev.prevent_default();
                    }
                    just_focused.set(false);
                }
                on:keydown=on_key
                on:blur=on_blur
            />
            <Show when=move || open.get()>
                <ul
                    class="ui-combo__list"
                    id=list_id.clone()
                    role="listbox"
                    // Keep the focus in the field when the list is touched; the click then chooses.
                    on:mousedown=move |ev| ev.prevent_default()
                >
                    {
                        let option_id = option_id_view.clone();
                        move || {
                            let list = options.get();
                            let typed = !query.with(|q| q.trim().is_empty());
                            let mut items = list
                                .into_iter()
                                .enumerate()
                                .map(|(i, o)| {
                                    let oid = option_id(i);
                                    let initials = o.initials.clone();
                                    let label = o.label.clone();
                                    view! {
                                        <li
                                            id=oid
                                            class="ui-combo__option"
                                            role="option"
                                            aria-selected=move || (active.get() == Some(i)).to_string()
                                            data-active=move || (active.get() == Some(i)).then_some("")
                                            data-divider=o.divider.then_some("")
                                            on:mouseenter=move |_| active.set(Some(i))
                                            on:click=move |_| choose(i)
                                        >
                                            {initials.map(|n| view! { <Avatar initials=n /> })}
                                            <span class="ui-combo__label">{label}</span>
                                        </li>
                                    }
                                    .into_any()
                                })
                                .collect::<Vec<_>>();
                            if typed
                                && items.is_empty()
                                && !busy.get()
                                && let Some(text) = empty_text.get_value()
                            {
                                items.push(
                                    view! { <li class="ui-combo__hint" role="presentation">{text}</li> }
                                        .into_any(),
                                );
                            }
                            if busy.get()
                                && let Some(text) = busy_text.get_value()
                            {
                                items.push(
                                    view! { <li class="ui-combo__hint" role="presentation">{text}</li> }
                                        .into_any(),
                                );
                            }
                            items
                        }
                    }
                </ul>
            </Show>
        </div>
    }
}
