use super::*;

// ---------- pure parts ----------

#[test]
fn container_timeout() {
    assert_eq!(allowed_seconds(Some(30), 0), 30 * 60 + GRACE_S);
    assert_eq!(allowed_seconds(Some(30), 10), 40 * 60 + GRACE_S);
    assert_eq!(allowed_seconds(None, 0), DEFAULT_MAX_S + GRACE_S);
}

#[test]
fn hint_to_reachability_out_the_container() {
    let raw = |t: &str| cockpit_i18n::Msg::raw(t);
    let h = check_hint(
        true,
        false,
        false,
        raw("Der Container erreicht den Proxy nicht (ENOTFOUND)."),
    )
    .de();
    assert!(h.contains("Dein Mac erreicht den Proxy, der Container aber nicht"));
    assert!(h.contains("VPN") && h.contains("ENOTFOUND") && h.contains("Podman"));
    assert!(
        check_hint(false, false, false, raw("x"))
            .de()
            .contains("Weder dein Mac noch der Container")
    );
    assert!(
        check_hint(true, true, false, raw("Status 401"))
            .de()
            .contains("Key wird abgelehnt")
    );
    assert!(
        check_hint(true, true, true, raw(""))
            .de()
            .contains("auch aus dem Container erreichbar")
    );
}

#[test]
fn advice_at_build() {
    let z = |s: &str| vec![s.to_string()];
    assert!(
        build_hint(&z("write: no space left on device"))
            .unwrap()
            .de()
            .contains("Platz")
    );
    assert!(
        build_hint(&z("dial tcp: lookup registry-1.docker.io: no such host"))
            .unwrap()
            .de()
            .contains("Netz")
    );
    assert!(build_hint(&z("etwas ganz anderes")).is_none());
}

#[tokio::test]
async fn lines_are_limited_read() {
    let mut data = Vec::new();
    data.extend_from_slice(b"{\"t\":\"hints_arrived\"}\n");
    data.extend(std::iter::repeat_n(b'x', MAX_LINE * 3));
    data.extend_from_slice(b"\nletzte ohne Zeilenende");
    let mut r = BufReader::new(&data[..]);
    let mut buffer = Vec::new();
    assert!(
        matches!(read_line(&mut r, &mut buffer).await.unwrap(), Some(Read::Line(z)) if z.contains("hints"))
    );
    assert!(matches!(
        read_line(&mut r, &mut buffer).await.unwrap(),
        Some(Read::TooLarge)
    ));
    assert!(
        matches!(read_line(&mut r, &mut buffer).await.unwrap(), Some(Read::Line(z)) if z == "letzte ohne Zeilenende")
    );
    assert!(read_line(&mut r, &mut buffer).await.unwrap().is_none());
    assert!(
        buffer.capacity() < MAX_LINE * 2,
        "never more than one line of maximum is buffered"
    );
}

// ---------- review run ----------

#[test]
fn review_events_are_limited_and_only_count_in_review_mode() {
    use crate::agent_run::{Event, as_message, line_read};
    let long = "x".repeat(5_000);
    let line = format!(
        r#"{{"t":"review_comment","path":"a.rs","line":4294967295,"side":"RIGHT","text":"{long}","suggestion":"{long}"}}"#
    );
    let Ok(Event::ReviewComment {
        line: n,
        text,
        suggestion,
        ..
    }) = line_read(&line)
    else {
        panic!("unreadable");
    };
    assert!(n <= 10_000_000);
    assert!(text.chars().count() <= 2_000);
    assert!(suggestion.unwrap().chars().count() <= 2_000);
    let e = line_read(r#"{"t":"review_comment","path":"a.rs","line":3,"side":"left","text":"t"}"#)
        .unwrap();
    assert!(as_message(&e, Mode::Review).is_some());
    // Not in the other modes, and not with a side that is neither RIGHT nor LEFT.
    assert!(as_message(&e, Mode::Implement).is_none());
    assert!(as_message(&e, Mode::Plan).is_none());
    let bad = line_read(r#"{"t":"review_comment","path":"a.rs","line":3,"side":"UP","text":"t"}"#)
        .unwrap();
    assert!(as_message(&bad, Mode::Review).is_none());
    let sum = line_read(r#"{"t":"review_summary","text":"s"}"#).unwrap();
    assert!(as_message(&sum, Mode::Review).is_some() && as_message(&sum, Mode::Plan).is_none());
}

#[test]
fn only_safe_branch_names_are_cloned_for_a_review() {
    use crate::agent_git::review_branch_ok;
    for ok in ["feature/x", "fix-1.2", "claude/BAU-1-x", "user/a_b"] {
        assert!(review_branch_ok(ok), "{ok}");
    }
    for bad in [
        "",
        "-f",
        "--upload-pack=x",
        "a..b",
        "a b",
        "a;b",
        "/x",
        "x/",
        "x.lock",
        "$(x)",
    ] {
        assert!(!review_branch_ok(bad), "{bad}");
    }
}
