//! The inbox filter chosen last, remembered per browser (`localStorage`; nothing secret).
//! Reading and writing never fail the app: without storage the default applies.

use cockpit_core::Filter;
use leptos::prelude::*;

const KEY: &str = "cockpit-tasks-filter";

/// The filter chosen last; "Ich" until one was chosen.
pub fn get() -> Filter {
    window()
        .local_storage()
        .ok()
        .flatten()
        .and_then(|s| s.get_item(KEY).ok().flatten())
        .and_then(|id| Filter::from_id(&id))
        .unwrap_or(Filter::Mine)
}

pub fn set(filter: Filter) {
    if let Some(s) = window().local_storage().ok().flatten() {
        let _ = s.set_item(KEY, filter.id());
    }
}
