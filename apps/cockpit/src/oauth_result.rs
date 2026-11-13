//! Return from the OAuth sign-in: the backend redirects to `/` and appends result and
//! message to the address (`?oauth=ok|error&service=…&message=…`). The message never contains a
//! key. Here it is displayed and the address is tidied up.

use leptos::prelude::*;

use crate::store::Store;

/// Result from the query of an address: `(ok, message)`.
pub fn parse(search: &str) -> Option<(bool, String)> {
    let mut ok = None;
    let mut message = String::new();
    for pair in search.trim_start_matches('?').split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "oauth" => ok = Some(value == "ok"),
            "message" => message = decode(value),
            _ => {}
        }
    }
    ok.map(|ok| {
        let message = if message.is_empty() {
            if ok {
                "Angemeldet".to_string()
            } else {
                "Die Anmeldung ist fehlgeschlagen".to_string()
            }
        } else {
            message
        };
        (ok, message)
    })
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
    let Some((_, message)) = parse(&search) else {
        return;
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
mod tests {
    use super::*;

    #[test]
    fn reads_success_and_error() {
        assert_eq!(
            parse("?oauth=ok&service=github&message=Mit%20GitHub%20angemeldet"),
            Some((true, "Mit GitHub angemeldet".into()))
        );
        assert_eq!(
            parse("?oauth=error&service=atlassian&message=Site%20%C3%A4%20nicht+frei"),
            Some((false, "Site ä nicht frei".into()))
        );
        assert_eq!(parse("?oauth=ok"), Some((true, "Angemeldet".into())));
        assert_eq!(
            parse("?oauth=error"),
            Some((false, "Die Anmeldung ist fehlgeschlagen".into()))
        );
    }

    #[test]
    fn ignores_other_addresses() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("?x=1"), None);
    }

    #[test]
    fn survives_broken_encoding() {
        assert_eq!(parse("?oauth=ok&message=100%").unwrap().1, "100%");
        assert_eq!(parse("?oauth=ok&message=%zz").unwrap().1, "%zz");
    }
}
