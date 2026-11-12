//! Rückkehr von der Anmeldung per OAuth: Das Backend leitet nach `/` um und hängt Ergebnis und
//! Meldung an die Adresse (`?oauth=ok|fehler&dienst=…&meldung=…`). Die Meldung enthält nie einen
//! Schlüssel. Hier wird sie angezeigt und die Adresse aufgeräumt.

use leptos::prelude::*;

use crate::store::Store;

/// Ergebnis aus der Abfrage einer Adresse: `(ok, meldung)`.
pub fn parse(search: &str) -> Option<(bool, String)> {
    let mut ok = None;
    let mut message = String::new();
    for pair in search.trim_start_matches('?').split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "oauth" => ok = Some(value == "ok"),
            "meldung" => message = decode(value),
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

/// Prozentkodierung rückgängig machen (UTF-8). Unlesbares bleibt stehen.
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

/// Zeigt das Ergebnis und entfernt die Abfrage aus der Adresse, damit ein Neuladen es nicht wiederholt.
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
            parse("?oauth=ok&dienst=github&meldung=Mit%20GitHub%20angemeldet"),
            Some((true, "Mit GitHub angemeldet".into()))
        );
        assert_eq!(
            parse("?oauth=fehler&dienst=atlassian&meldung=Site%20%C3%A4%20nicht+frei"),
            Some((false, "Site ä nicht frei".into()))
        );
        assert_eq!(parse("?oauth=ok"), Some((true, "Angemeldet".into())));
        assert_eq!(
            parse("?oauth=fehler"),
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
        assert_eq!(parse("?oauth=ok&meldung=100%").unwrap().1, "100%");
        assert_eq!(parse("?oauth=ok&meldung=%zz").unwrap().1, "%zz");
    }
}
