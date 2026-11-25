//! Tests only: reads an answer of the API as German text. A message in a body is a `Msg` object
//! (`{"code": …, "args": …}`); the tests compare with its German rendering.

use cockpit_i18n::{Lang, Msg};
use serde_json::Value;

/// `value` with every message object replaced by its German text.
pub(crate) fn german_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let is_msg = map.get("code").is_some_and(Value::is_string)
                && map.keys().all(|k| k == "code" || k == "args");
            if is_msg && let Ok(m) = serde_json::from_value::<Msg>(Value::Object(map.clone())) {
                *value = Value::String(m.render(Lang::De));
                return;
            }
            map.values_mut().for_each(german_value);
        }
        Value::Array(items) => items.iter_mut().for_each(german_value),
        _ => {}
    }
}

/// The JSON of a body with its messages in German; a body that is no JSON stays as it is.
pub(crate) fn german_json(body: &str) -> Value {
    let mut value = serde_json::from_str(body).unwrap_or_else(|e| panic!("no JSON ({e}): {body}"));
    german_value(&mut value);
    value
}

/// The text of a body with its messages in German: for `body.contains("…")`.
pub(crate) fn german(body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(mut value) => {
            german_value(&mut value);
            value.to_string()
        }
        Err(_) => body.to_string(),
    }
}
