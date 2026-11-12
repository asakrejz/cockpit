//! Beim ersten Start, wenn noch nichts eingerichtet ist, führt Cockpit zum Einrichtungsassistenten.
//! Gemerkt wird nur, dass er einmal gezeigt wurde (kein Schlüssel, keine Zugangsdaten).

use cockpit_core::einrichten::nichts_eingerichtet;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::route::Route;
use crate::server;
use crate::store::{Store, navigate};

const MERKER: &str = "cockpit-einrichtung-gezeigt";

fn schon_gezeigt() -> bool {
    window()
        .local_storage()
        .ok()
        .flatten()
        .and_then(|s| s.get_item(MERKER).ok().flatten())
        .is_some()
}

fn merken() {
    if let Some(s) = window().local_storage().ok().flatten() {
        let _ = s.set_item(MERKER, "1");
    }
}

/// Prüft einmal, sobald der Server geantwortet hat. Greift nur, wenn man auf der Startseite
/// steht – wer einen Link zu einer anderen Seite öffnet, wird nicht umgeleitet.
pub fn pruefen(store: Store) {
    if schon_gezeigt() {
        return;
    }
    let erledigt = StoredValue::new(false);
    Effect::new(move |_| {
        let Some(health) = store.health.get() else {
            return;
        };
        if erledigt.get_value() {
            return;
        }
        erledigt.set_value(true);
        spawn_local(async move {
            let Ok(einrichtung) = server::fetch_einrichtung().await else {
                return;
            };
            let leer = nichts_eingerichtet(
                &einrichtung,
                &health.anmeldung.jira,
                &health.anmeldung.github,
                &health.endpunkt,
            );
            if leer && matches!(store.route.get_untracked(), Route::Inbox(_)) {
                merken();
                navigate(&Route::Setup);
            }
        });
    });
}
