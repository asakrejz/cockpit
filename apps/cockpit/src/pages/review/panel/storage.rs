//! "Viewed" is kept in the browser, per pull request. It is a convenience only: if the
//! storage is blocked or empty, nothing is marked.

use cockpit_core::{PrKey, Viewed};
use leptos::prelude::window;

fn key(pr: &PrKey) -> String {
    format!("cockpit-viewed:{pr}")
}

pub(super) fn load(pr: &PrKey) -> Viewed {
    window()
        .local_storage()
        .ok()
        .flatten()
        .and_then(|s| s.get_item(&key(pr)).ok().flatten())
        .map(|json| Viewed::parse(&json))
        .unwrap_or_default()
}

pub(super) fn save(pr: &PrKey, viewed: &Viewed) {
    if let Some(s) = window().local_storage().ok().flatten() {
        let _ = s.set_item(&key(pr), &viewed.to_json());
    }
}
