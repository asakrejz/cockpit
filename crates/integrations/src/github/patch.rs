//! Parser für Unified-Diff-Patches, wie GitHub sie im Feld `patch` liefert.

use cockpit_core::{DiffLine, LineKind};

/// `@@ -a,b +c,d @@ …` → `(a, b, c, d)`; fehlende Längen bedeuten 1.
fn hunk_kopf(zeile: &str) -> Option<(u32, u32, u32, u32)> {
    let rest = zeile.strip_prefix("@@ -")?;
    let (alt, rest) = rest.split_once(" +")?;
    let neu = rest.split(" @@").next()?;
    let teil = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (a, b) = teil(alt)?;
    let (c, d) = teil(neu)?;
    Some((a, b, c, d))
}

/// Wandelt einen Patch in Diff-Zeilen. Zeilennummern wie in `cockpit-core`: neue und
/// unveränderte Zeilen zählen im neuen Stand, entfernte im alten.
///
/// Die `@@`-Köpfe selbst werden nicht als Zeilen übernommen; zwischen zwei Abschnitten
/// springt die Nummer entsprechend. Zeilen außerhalb eines Abschnitts (Dateikopf,
/// „\ No newline at end of file“) werden ignoriert.
pub fn parse_patch(patch: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let (mut alt, mut neu) = (0u32, 0u32);
    let (mut alt_rest, mut neu_rest) = (0u32, 0u32);

    for zeile in patch.lines() {
        if zeile.starts_with("@@") {
            if let Some((a, b, c, d)) = hunk_kopf(zeile) {
                (alt, alt_rest, neu, neu_rest) = (a, b, c, d);
            } else {
                // Kaputter Kopf: lieber nichts anzeigen als falsche Nummern.
                (alt_rest, neu_rest) = (0, 0);
            }
            continue;
        }
        if zeile.starts_with('\\') || (alt_rest == 0 && neu_rest == 0) {
            continue;
        }
        // Eine leere Zeile ist eine Kontextzeile, deren Leerzeichen verloren ging.
        let (zeichen, text) = zeile.split_at(zeile.chars().next().map_or(0, char::len_utf8));
        match zeichen {
            "+" if neu_rest > 0 => {
                out.push(DiffLine {
                    no: neu,
                    kind: LineKind::Add,
                    text: text.into(),
                });
                neu += 1;
                neu_rest -= 1;
            }
            "-" if alt_rest > 0 => {
                out.push(DiffLine {
                    no: alt,
                    kind: LineKind::Del,
                    text: text.into(),
                });
                alt += 1;
                alt_rest -= 1;
            }
            " " | "" if alt_rest > 0 && neu_rest > 0 => {
                out.push(DiffLine {
                    no: neu,
                    kind: LineKind::Context,
                    text: text.into(),
                });
                alt += 1;
                neu += 1;
                alt_rest -= 1;
                neu_rest -= 1;
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zeile(no: u32, kind: LineKind, text: &str) -> DiffLine {
        DiffLine {
            no,
            kind,
            text: text.into(),
        }
    }

    #[test]
    fn einfacher_abschnitt() {
        let patch = "@@ -10,4 +10,4 @@ fn x()\n a\n-b\n+B\n c";
        assert_eq!(
            parse_patch(patch),
            vec![
                zeile(10, LineKind::Context, "a"),
                zeile(11, LineKind::Del, "b"),
                zeile(11, LineKind::Add, "B"),
                zeile(12, LineKind::Context, "c"),
            ]
        );
    }

    #[test]
    fn mehrere_abschnitte_und_neue_datei() {
        let patch = "@@ -1,2 +1,3 @@\n x\n+neu\n y\n@@ -20 +21 @@\n-alt\n+neu2";
        let z = parse_patch(patch);
        assert_eq!(z.len(), 5);
        assert_eq!(z[1], zeile(2, LineKind::Add, "neu"));
        assert_eq!(z[3], zeile(20, LineKind::Del, "alt"));
        assert_eq!(z[4], zeile(21, LineKind::Add, "neu2"));

        let neu = "@@ -0,0 +1,2 @@\n+eins\n+zwei";
        assert_eq!(
            parse_patch(neu),
            vec![
                zeile(1, LineKind::Add, "eins"),
                zeile(2, LineKind::Add, "zwei")
            ]
        );
    }

    #[test]
    fn inhalt_der_wie_ein_kopf_aussieht_bleibt_inhalt() {
        // Entfernte Zeile "-- kommentar" beginnt mit "--"; das ist keine Kopfzeile.
        let patch = "@@ -1,2 +1,1 @@\n--- kommentar\n ende";
        let z = parse_patch(patch);
        assert_eq!(z[0], zeile(1, LineKind::Del, "-- kommentar"));
        assert_eq!(z[1], zeile(1, LineKind::Context, "ende"));
    }

    #[test]
    fn kein_zeilenumbruch_hinweis_und_leere_kontextzeile() {
        let patch = "@@ -1,3 +1,3 @@\n a\n\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file";
        let z = parse_patch(patch);
        assert_eq!(
            z,
            vec![
                zeile(1, LineKind::Context, "a"),
                zeile(2, LineKind::Context, ""),
                zeile(3, LineKind::Del, "b"),
                zeile(3, LineKind::Add, "c"),
            ]
        );
    }

    #[test]
    fn umlaute_und_dateikopf() {
        let patch = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-größe\n+Größe";
        let z = parse_patch(patch);
        assert_eq!(
            z,
            vec![
                zeile(1, LineKind::Del, "größe"),
                zeile(1, LineKind::Add, "Größe")
            ]
        );
    }

    #[test]
    fn leer_und_kaputt() {
        assert!(parse_patch("").is_empty());
        assert!(parse_patch("@@ kaputt @@\n+x").is_empty());
        // Mehr Zeilen als angekündigt: der Rest wird ignoriert.
        assert_eq!(parse_patch("@@ -1 +1 @@\n+a\n+b").len(), 1);
    }
}
