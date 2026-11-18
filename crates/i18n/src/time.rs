//! Relative times: "5 minutes ago" / "vor 5 Minuten".

use crate::{Key, Lang, text};

/// How long ago `seconds` is. Under a minute is "just now".
pub fn ago(lang: Lang, seconds: u64) -> String {
    let (key, n) = match seconds {
        0..=59 => return text(lang, Key::TimeJustNow, &[]),
        60..=3599 => (Key::TimeAgoMinutes, seconds / 60),
        3600..=86_399 => (Key::TimeAgoHours, seconds / 3600),
        _ => (Key::TimeAgoDays, seconds / 86_400),
    };
    text(lang, key, &[("n", n.to_string())])
}

/// Like [`ago`], from two points in time in milliseconds. A point in the future counts as now.
pub fn ago_ms(lang: Lang, then_ms: u64, now_ms: u64) -> String {
    ago(lang, now_ms.saturating_sub(then_ms) / 1000)
}
