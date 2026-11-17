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
    let h = check_hint(
        true,
        false,
        false,
        "Der Container erreicht den Proxy nicht (ENOTFOUND).",
    );
    assert!(h.contains("Dein Mac erreicht den Proxy, der Container aber nicht"));
    assert!(h.contains("VPN") && h.contains("ENOTFOUND") && h.contains("Podman"));
    assert!(check_hint(false, false, false, "x").contains("Weder dein Mac noch der Container"));
    assert!(check_hint(true, true, false, "Status 401").contains("Key wird abgelehnt"));
    assert!(check_hint(true, true, true, "").contains("auch aus dem Container erreichbar"));
}

#[test]
fn advice_at_build() {
    let z = |s: &str| vec![s.to_string()];
    assert!(
        build_hint(&z("write: no space left on device"))
            .unwrap()
            .contains("Platz")
    );
    assert!(
        build_hint(&z("dial tcp: lookup registry-1.docker.io: no such host"))
            .unwrap()
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
        "es wird nie mehr als eine Zeile Maximum gepuffert"
    );
}
