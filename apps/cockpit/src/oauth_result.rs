//! Return from the OAuth sign-in: the backend redirects to `/` and appends result and
//! message to the address (`?oauth=ok|error&service=…&msg=…`, the message as JSON). The message
//! never contains a key. Here it is displayed and the address is tidied up.

use feature_ui::t;
use leptos::prelude::*;

use crate::store::Store;

/// Result from the query of an address: `(ok, message)`; the message is `None` if the backend
/// sent none (then [`announce`] uses a standard text).
pub fn parse(search: &str) -> Option<(bool, Option<cockpit_i18n::Msg>)> {
    let mut ok = None;
    let mut message = None;
    for pair in search.trim_start_matches('?').split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "oauth" => ok = Some(value == "ok"),
            "msg" => message = serde_json::from_str(&decode(value)).ok(),
            _ => {}
        }
    }
    ok.map(|ok| (ok, message))
}

/// Undo percent encoding (UTF-8). Unreadable parts stay as they are.
fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Shows the result and removes the query from the address so that a reload does not repeat it.
pub fn announce(store: Store) {
    let Ok(search) = window().location().search() else {
        return;
    };
    let Some((ok, message)) = parse(&search) else {
        return;
    };
    let message = if let Some(coded) = message {
        coded.render(feature_ui::lang_untracked())
    } else if ok {
        t!(OauthSignedIn)
    } else {
        t!(OauthSignInFailed)
    };
    store.notify(message);
    if let (Ok(history), Ok(hash)) = (window().history(), window().location().hash()) {
        let _ = history.replace_state_with_url(
            &leptos::wasm_bindgen::JsValue::NULL,
            "",
            Some(&format!("/{hash}")),
        );
    }
}

#[cfg(test)]
mod tests;
