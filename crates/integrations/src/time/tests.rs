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
