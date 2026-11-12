//! Benachrichtigungen im Browser: Erlaubnis, Systemmeldung und Uhrzeit.
//!
//! Was gemeldet wird und wann, entscheidet `cockpit_core::notify`. Hier steht nur, was den
//! Browser braucht (Notification-API, Sichtbarkeit des Tabs, Ortszeit). Gefragt wird nach der
//! Erlaubnis nie von selbst, sondern nur auf Knopfdruck.

use cockpit_core::NotifyEvent;
use leptos::prelude::*;
use web_sys::{
    Notification, NotificationOptions, NotificationPermission, VisibilityState,
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
};

use crate::{route::Route, store::navigate_hash};

/// Wie es um die Erlaubnis des Browsers steht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// Dieser Browser kennt die Notification-API nicht.
    Unsupported,
    /// Noch nicht gefragt.
    Ask,
    Granted,
    Denied,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Permission::Unsupported => "Dieser Browser unterstützt keine Benachrichtigungen",
            Permission::Ask => "Noch nicht erlaubt",
            Permission::Granted => "Erlaubt",
            Permission::Denied => "Abgelehnt – in den Browser-Einstellungen änderbar",
        }
    }
}

/// Aktueller Stand. Ohne die API (ältere Browser) heißt es „nicht unterstützt“.
pub fn permission() -> Permission {
    let known =
        js_sys::Reflect::has(&window(), &JsValue::from_str("Notification")).unwrap_or(false);
    if !known {
        return Permission::Unsupported;
    }
    match Notification::permission() {
        NotificationPermission::Granted => Permission::Granted,
        NotificationPermission::Denied => Permission::Denied,
        _ => Permission::Ask,
    }
}

/// Fragt den Browser nach der Erlaubnis. Nur aus einem Klick aufrufen.
/// `done` bekommt danach den neuen Stand.
pub fn request(done: impl Fn(Permission) + 'static) {
    if permission() != Permission::Ask {
        done(permission());
        return;
    }
    if let Ok(promise) = Notification::request_permission() {
        let on_answer = Closure::new(move |_answer: JsValue| done(permission()));
        let _ = promise.then(&on_answer);
        // Der Browser ruft das später auf; die Funktion muss so lange leben.
        on_answer.forget();
    }
}

/// Ist der Tab gerade zu sehen?
pub fn foreground() -> bool {
    document().visibility_state() == VisibilityState::Visible
}

/// Ortszeit in Minuten seit Mitternacht – für die Ruhezeiten.
pub fn minute_of_day() -> u16 {
    let now = js_sys::Date::new_0();
    (now.get_hours() * 60 + now.get_minutes()) as u16
}

/// Zeigt eine Systemmeldung, wenn der Browser es erlaubt. Ein Klick darauf holt den Tab
/// nach vorn und öffnet das Ziel des Ereignisses.
pub fn show_system(event: &NotifyEvent) {
    if permission() != Permission::Granted {
        return;
    }
    let options = NotificationOptions::new();
    options.set_body(&event.text);
    // Gleiche Ereignisse ersetzen sich, statt sich zu stapeln.
    options.set_tag(&format!("{}{}", event.reason.id(), event.route));
    let Ok(note) = Notification::new_with_options(&event.title, &options) else {
        return;
    };
    let route = event.route.clone();
    let shown = note.clone();
    let on_click = Closure::once_into_js(move || {
        let _ = window().focus();
        navigate_hash(&route);
        shown.close();
    });
    note.set_onclick(Some(on_click.unchecked_ref()));
}

/// Die Seite, auf der man alles ausprobieren kann.
pub fn settings_route() -> String {
    Route::Notifications.hash()
}
