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
