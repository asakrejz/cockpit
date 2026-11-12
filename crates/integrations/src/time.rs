//! Read timestamps (ISO 8601) and show them as „vor 5 Min.“ – without a time crate.

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

/// „gerade eben“, „vor 5 Min.“, „vor 3 Std.“, „gestern“, „vor 4 Tagen“, otherwise the date.
/// If the timestamp is not readable, it comes back unchanged.
pub fn relative(iso: &str, now: i64) -> String {
    let Some(t) = parse_iso(iso) else {
        return iso.to_string();
    };
    let d = (now - t).max(0);
    match d {
        0..=59 => "gerade eben".into(),
        60..=3599 => format!("vor {} Min.", d / 60),
        3600..=86_399 => format!("vor {} Std.", d / 3600),
        86_400..=172_799 => "gestern".into(),
        172_800..=1_209_599 => format!("vor {} Tagen", d / 86_400),
        _ => iso[..10].to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEUJAHR_2024: i64 = 1_704_067_200;

    #[test]
    fn reads_timestamp() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2024-01-01T00:00:00Z"), Some(NEUJAHR_2024));
        assert_eq!(parse_iso("2000-03-01T00:00:00Z"), Some(951_868_800));
        // Leap year: 29 February
        assert_eq!(
            parse_iso("2024-03-01T00:00:00Z"),
            Some(NEUJAHR_2024 + 60 * 86_400)
        );
    }

    #[test]
    fn reads_timezones_and_fractions() {
        let utc = parse_iso("2024-05-14T07:12:33Z").unwrap();
        assert_eq!(parse_iso("2024-05-14T09:12:33.456+0200"), Some(utc));
        assert_eq!(parse_iso("2024-05-14T09:12:33+02:00"), Some(utc));
        assert_eq!(parse_iso("2024-05-14T05:42:33-01:30"), Some(utc));
    }

    #[test]
    fn rejects_nonsense_ab() {
        assert_eq!(parse_iso("gestern"), None);
        assert_eq!(parse_iso("2024-13-01T00:00:00Z"), None);
        assert_eq!(parse_iso("2024-05-14T09:12:33+2"), None);
    }

    #[test]
    fn relative_display() {
        let t = "2024-05-14T10:00:00Z";
        let n = parse_iso(t).unwrap();
        assert_eq!(relative(t, n + 5), "gerade eben");
        assert_eq!(relative(t, n + 300), "vor 5 Min.");
        assert_eq!(relative(t, n + 3 * 3600), "vor 3 Std.");
        assert_eq!(relative(t, n + 30 * 3600), "gestern");
        assert_eq!(relative(t, n + 4 * 86_400), "vor 4 Tagen");
        assert_eq!(relative(t, n + 30 * 86_400), "2024-05-14");
        assert_eq!(relative("unlesbar", n), "unlesbar");
        // Clocks that run slightly ahead never yield negative values.
        assert_eq!(relative(t, n - 10), "gerade eben");
    }
}
