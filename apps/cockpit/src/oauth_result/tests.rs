use super::*;
use cockpit_i18n::{Lang, Msg, msg};

/// The query the backend appends: the message as JSON, percent-encoded.
fn query(state: &str, message: &Msg) -> String {
    let json = serde_json::to_string(message).unwrap();
    let mut encoded = String::new();
    for b in json.bytes() {
        if b.is_ascii_alphanumeric() {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("?oauth={state}&service=github&msg={encoded}")
}

#[test]
fn reads_success_and_error() {
    let ok = msg!(OauthSignedIn);
    let (state, message) = parse(&query("ok", &ok)).unwrap();
    assert!(state);
    assert_eq!(message.unwrap().render(Lang::En), ok.render(Lang::En));

    let failed = Msg::raw("Site ä nicht frei");
    let (state, message) = parse(&query("error", &failed)).unwrap();
    assert!(!state);
    assert_eq!(message, Some(failed));

    assert_eq!(parse("?oauth=ok"), Some((true, None)));
    assert_eq!(parse("?oauth=error"), Some((false, None)));
}

#[test]
fn ignores_other_addresses() {
    assert_eq!(parse(""), None);
    assert_eq!(parse("?x=1"), None);
}

#[test]
fn survives_broken_encoding() {
    assert_eq!(parse("?oauth=ok&msg=100%"), Some((true, None)));
    assert_eq!(parse("?oauth=ok&msg=%zz"), Some((true, None)));
}
