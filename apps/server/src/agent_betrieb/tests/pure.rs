use super::*;

// ---------- reine Teile ----------

#[test]
fn zeitlimit_des_containers() {
    assert_eq!(erlaubte_sekunden(Some(30), 0), 30 * 60 + NACHSPIELZEIT_S);
    assert_eq!(erlaubte_sekunden(Some(30), 10), 40 * 60 + NACHSPIELZEIT_S);
    assert_eq!(
        erlaubte_sekunden(None, 0),
        STANDARD_MAXIMAL_S + NACHSPIELZEIT_S
    );
}

#[test]
fn hinweis_zur_erreichbarkeit_aus_dem_container() {
    let h = pruefung_hinweis(
        true,
        false,
        false,
        "Der Container erreicht den Proxy nicht (ENOTFOUND).",
    );
    assert!(h.contains("Dein Mac erreicht den Proxy, der Container aber nicht"));
    assert!(h.contains("VPN") && h.contains("ENOTFOUND") && h.contains("podman machine"));
    assert!(
        pruefung_hinweis(false, false, false, "x").contains("Weder dein Mac noch der Container")
    );
    assert!(pruefung_hinweis(true, true, false, "Status 401").contains("Key wird abgelehnt"));
    assert!(pruefung_hinweis(true, true, true, "").contains("auch aus dem Container erreichbar"));
}

#[test]
fn ratschlaege_beim_bauen() {
    let z = |s: &str| vec![s.to_string()];
    assert!(
        bau_hinweis(&z("write: no space left on device"))
            .unwrap()
            .contains("Platz")
    );
    assert!(
        bau_hinweis(&z("dial tcp: lookup registry-1.docker.io: no such host"))
            .unwrap()
            .contains("Netz")
    );
    assert!(bau_hinweis(&z("etwas ganz anderes")).is_none());
}

#[tokio::test]
async fn zeilen_werden_begrenzt_gelesen() {
    let mut daten = Vec::new();
    daten.extend_from_slice(b"{\"t\":\"hinweise_angekommen\"}\n");
    daten.extend(std::iter::repeat_n(b'x', MAX_ZEILE * 3));
    daten.extend_from_slice(b"\nletzte ohne Zeilenende");
    let mut r = BufReader::new(&daten[..]);
    let mut puffer = Vec::new();
    assert!(
        matches!(lies_zeile(&mut r, &mut puffer).await.unwrap(), Some(Gelesen::Zeile(z)) if z.contains("hinweise"))
    );
    assert!(matches!(
        lies_zeile(&mut r, &mut puffer).await.unwrap(),
        Some(Gelesen::ZuGross)
    ));
    assert!(
        matches!(lies_zeile(&mut r, &mut puffer).await.unwrap(), Some(Gelesen::Zeile(z)) if z == "letzte ohne Zeilenende")
    );
    assert!(lies_zeile(&mut r, &mut puffer).await.unwrap().is_none());
    assert!(
        puffer.capacity() < MAX_ZEILE * 2,
        "es wird nie mehr als eine Zeile Maximum gepuffert"
    );
}
