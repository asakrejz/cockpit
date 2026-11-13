//! On the first start, when nothing is set up yet, Cockpit leads to the setup assistant.
//! Only the fact that it was shown once is remembered (no key, no credentials).

use cockpit_core::setup::nothing_set_up;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::route::Route;
use crate::server;
use crate::store::{Store, navigate};

const MARKER: &str = "cockpit-setup-shown";
/// The older key (kept readable so the assistant does not appear a second time).
const OLD_MARKER: &str = "cockpit-einrichtung-gezeigt";

fn already_shown() -> bool {
    window()
        .local_storage()
        .ok()
        .flatten()
        .and_then(|s| {
            s.get_item(MARKER)
                .ok()
                .flatten()
                .or_else(|| s.get_item(OLD_MARKER).ok().flatten())
        })
        .is_some()
}

fn remember() {
    if let Some(s) = window().local_storage().ok().flatten() {
        let _ = s.set_item(MARKER, "1");
    }
}

/// Checks once as soon as the server has answered. Only acts if you are on the start page
/// – whoever opens a link to another page is not redirected.
pub fn check(store: Store) {
    if already_shown() {
        return;
    }
    let done = StoredValue::new(false);
    Effect::new(move |_| {
        let Some(health) = store.health.get() else {
            return;
        };
        if done.get_value() {
            return;
        }
        done.set_value(true);
        spawn_local(async move {
            let Ok(setup) = server::fetch_setup().await else {
                return;
            };
            let empty = nothing_set_up(
                &setup,
                &health.sign_in.jira,
                &health.sign_in.github,
                &health.endpoint,
            );
            if empty && matches!(store.route.get_untracked(), Route::Inbox(_)) {
                remember();
                navigate(&Route::Setup);
            }
        });
    });
}
