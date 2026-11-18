//! Read timestamps (ISO 8601) and show them as "5 minutes ago" – without a time crate.

use cockpit_i18n::{Key, Lang};

/// Days since 1970-01-01 for a date in the Gregorian calendar.
fn days_since_1970(year: i64, month: i64, tag: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5 + tag - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn number(s: &str, from: usize, until: usize) -> Option<i64> {
    s.get(from..until)?.parse().ok()
}

/// Reads `2024-05-14T09:12:33Z`, `…33.456+0200` or `…33+02:00` as seconds since 1970 (UTC).
pub fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !(b[10] == b'T' || b[10] == b' ') {
        return None;
    }
    let (j, mo, t) = (number(s, 0, 4)?, number(s, 5, 7)?, number(s, 8, 10)?);
    let (h, mi, se) = (number(s, 11, 13)?, number(s, 14, 16)?, number(s, 17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&t) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        rest = r.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest.as_bytes() {
        [] | [b'Z'] => 0,
        [vz @ (b'+' | b'-'), ..] => {
            let digits: String = rest[1..].chars().filter(char::is_ascii_digit).collect();
            if digits.len() != 4 {
                return None;
            }
            let sec = number(&digits, 0, 2)? * 3600 + number(&digits, 2, 4)? * 60;
            if *vz == b'+' { sec } else { -sec }
        }
        _ => return None,
    };
    Some(days_since_1970(j, mo, t) * 86_400 + h * 3600 + mi * 60 + se - offset)
}

/// How long ago `iso` is, in `lang`: "just now", "5 minutes ago", "3 hours ago", "yesterday",
/// "4 days ago", otherwise the date. If the timestamp is not readable, it comes back unchanged.
pub fn relative(iso: &str, now: i64, lang: Lang) -> String {
    let Some(t) = parse_iso(iso) else {
        return iso.to_string();
    };
    let d = (now - t).max(0);
    match d {
        86_400..=172_799 => cockpit_i18n::text(lang, Key::TimeYesterday, &[]),
        1_209_600.. => iso[..10].to_string(),
        _ => cockpit_i18n::ago(lang, d as u64),
    }
}

#[cfg(test)]
mod tests;
