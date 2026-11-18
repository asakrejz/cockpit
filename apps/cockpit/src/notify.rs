//! Notifications in the browser: permission, system message and time of day.
//!
//! What is reported and when is decided by `cockpit_core::notify`. Only what the
//! browser needs is here (Notification API, visibility of the tab, local time). The
//! permission is never asked for on its own, only on a button press.

use cockpit_core::NotifyEvent;
use leptos::prelude::*;
use web_sys::{
    Notification, NotificationOptions, NotificationPermission, VisibilityState,
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
};

use crate::{route::Route, store::navigate_hash};

/// Name of the browser API on `window`.
const API_NAME: &str = "Notification";

/// How the browser's permission stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// This browser does not know the Notification API.
    Unsupported,
    /// Not asked yet.
    Ask,
    Granted,
    Denied,
}

/// Current state. Without the API (older browsers) it says "not supported".
pub fn permission() -> Permission {
    let known = js_sys::Reflect::has(&window(), &JsValue::from_str(API_NAME)).unwrap_or(false);
    if !known {
        return Permission::Unsupported;
    }
    match Notification::permission() {
        NotificationPermission::Granted => Permission::Granted,
        NotificationPermission::Denied => Permission::Denied,
        _ => Permission::Ask,
    }
}

/// Asks the browser for permission. Only call from a click.
/// `done` then gets the new state.
pub fn request(done: impl Fn(Permission) + 'static) {
    if permission() != Permission::Ask {
        done(permission());
        return;
    }
    if let Ok(promise) = Notification::request_permission() {
        let on_answer = Closure::new(move |_answer: JsValue| done(permission()));
        let _ = promise.then(&on_answer);
        // The browser calls this later; the function has to live that long.
        on_answer.forget();
    }
}

/// Is the tab visible right now?
pub fn foreground() -> bool {
    document().visibility_state() == VisibilityState::Visible
}

/// Local time in minutes since midnight – for the quiet hours.
pub fn minute_of_day() -> u16 {
    let now = js_sys::Date::new_0();
    (now.get_hours() * 60 + now.get_minutes()) as u16
}

/// Shows a system message if the browser allows it. A click on it brings the tab
/// to the front and opens the target of the event.
pub fn show_system(event: &NotifyEvent) {
    if permission() != Permission::Granted {
        return;
    }
    let options = NotificationOptions::new();
    let lang = feature_ui::lang_untracked();
    options.set_body(&event.text.render(lang));
    // Equal events replace each other instead of piling up.
    options.set_tag(&format!("{}{}", event.reason.id(), event.route));
    let Ok(note) = Notification::new_with_options(&event.title.render(lang), &options) else {
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

/// The page where you can try everything out.
pub fn settings_route() -> String {
    Route::Notifications.hash()
}
