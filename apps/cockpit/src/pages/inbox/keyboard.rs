use super::*;

/// Erstes Element, das zum Selektor passt.
pub(super) fn query_html(selector: &str) -> Option<HtmlElement> {
    document()
        .query_selector(selector)
        .ok()
        .flatten()
        .and_then(|el| el.dyn_into().ok())
}

/// Setzt den Fokus auf ein Eingabefeld und markiert dessen Inhalt.
pub(super) fn focus_field(selector: &str) {
    if let Some(el) = query_html(selector) {
        let _ = el.focus();
        if let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() {
            input.select();
        }
    }
}

/// Tippt der Nutzer gerade in ein Eingabefeld? Dann gehören die Tasten dem Feld.
pub(super) fn is_typing(event: &KeyboardEvent) -> bool {
    event
        .target()
        .and_then(|t| t.dyn_into::<HtmlElement>().ok())
        .is_some_and(|el| {
            matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                || el.is_content_editable()
        })
}

/// Ein Tastendruck in der Inbox: ↑/↓ und j/k wechseln den Eintrag, `e` erledigt ihn,
/// `r` springt ins Antwortfeld, `s` zu den Möglichkeiten zum Zurückstellen, `u` holt einen
/// zurückgestellten Eintrag zurück, `/` springt in die Suche.
pub(super) fn on_key(
    store: Store,
    items: Memo<Vec<Item>>,
    current: Memo<Option<Item>>,
    event: KeyboardEvent,
) {
    if event.ctrl_key() || event.meta_key() || event.alt_key() {
        return;
    }
    if is_typing(&event) {
        // Escape verlässt das Feld und gibt die Kürzel wieder frei.
        if event.key() == "Escape"
            && let Some(el) = event
                .target()
                .and_then(|t| t.dyn_into::<HtmlElement>().ok())
        {
            let _ = el.blur();
        }
        return;
    }
    let step: isize = match event.key().as_str() {
        "ArrowDown" | "j" => 1,
        "ArrowUp" | "k" => -1,
        "e" => {
            if let Some(item) = current.get_untracked() {
                let _ = store.dispatch(Action::MarkDone {
                    item: item.id.clone(),
                });
            }
            return;
        }
        "r" => {
            event.prevent_default();
            focus_field(".ck-reply textarea");
            return;
        }
        "s" => {
            event.prevent_default();
            if let Some(button) = query_html(".ck-snooze button") {
                let _ = button.focus();
            }
            return;
        }
        "u" => {
            if let Some(item) = current.get_untracked() {
                let _ = store.dispatch(Action::Unsnooze { item: item.id });
            }
            return;
        }
        "/" => {
            event.prevent_default();
            focus_field(".ck-search input");
            return;
        }
        _ => return,
    };
    event.prevent_default();
    let list = items.get_untracked();
    let Some(now) = current.get_untracked() else {
        return;
    };
    let at = list.iter().position(|i| i.id == now.id).unwrap_or(0);
    if let Some(next) = at.checked_add_signed(step).and_then(|n| list.get(n)) {
        store.selected.set(next.id.clone());
    }
}
