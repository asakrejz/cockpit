//! Parser für Unified-Diff-Patches, wie GitHub sie im Feld `patch` liefert.

use cockpit_core::{DiffLine, LineKind};

/// `@@ -a,b +c,d @@ …` → `(a, b, c, d)`; fehlende Längen bedeuten 1.
fn hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let (alt, rest) = rest.split_once(" +")?;
    let again = rest.split(" @@").next()?;
    let part = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (a, b) = part(alt)?;
    let (c, d) = part(again)?;
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
    let (mut alt, mut again) = (0u32, 0u32);
    let (mut alt_rest, mut again_rest) = (0u32, 0u32);

    for line in patch.lines() {
        if line.starts_with("@@") {
            if let Some((a, b, c, d)) = hunk_header(line) {
                (alt, alt_rest, again, again_rest) = (a, b, c, d);
            } else {
                // Kaputter Kopf: lieber nichts anzeigen als falsche Nummern.
                (alt_rest, again_rest) = (0, 0);
            }
            continue;
        }
        if line.starts_with('\\') || (alt_rest == 0 && again_rest == 0) {
            continue;
        }
        // Eine leere Zeile ist eine Kontextzeile, deren Leerzeichen verloren ging.
        let (chars, text) = line.split_at(line.chars().next().map_or(0, char::len_utf8));
        match chars {
            "+" if again_rest > 0 => {
                out.push(DiffLine {
                    no: again,
                    kind: LineKind::Add,
                    text: text.into(),
                });
                again += 1;
                again_rest -= 1;
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
            " " | "" if alt_rest > 0 && again_rest > 0 => {
                out.push(DiffLine {
                    no: again,
                    kind: LineKind::Context,
                    text: text.into(),
                });
                alt += 1;
                again += 1;
                alt_rest -= 1;
                again_rest -= 1;
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(no: u32, kind: LineKind, text: &str) -> DiffLine {
        DiffLine {
            no,
            kind,
            text: text.into(),
        }
    }

    #[test]
    fn simple_section() {
        let patch = "@@ -10,4 +10,4 @@ fn x()\n a\n-b\n+B\n c";
        assert_eq!(
            parse_patch(patch),
            vec![
                line(10, LineKind::Context, "a"),
                line(11, LineKind::Del, "b"),
                line(11, LineKind::Add, "B"),
                line(12, LineKind::Context, "c"),
            ]
        );
    }

    #[test]
    fn several_sections_and_new_file() {
        let patch = "@@ -1,2 +1,3 @@\n x\n+neu\n y\n@@ -20 +21 @@\n-alt\n+neu2";
        let z = parse_patch(patch);
        assert_eq!(z.len(), 5);
        assert_eq!(z[1], line(2, LineKind::Add, "neu"));
        assert_eq!(z[3], line(20, LineKind::Del, "alt"));
        assert_eq!(z[4], line(21, LineKind::Add, "neu2"));

        let again = "@@ -0,0 +1,2 @@\n+eins\n+zwei";
        assert_eq!(
            parse_patch(again),
            vec![
                line(1, LineKind::Add, "eins"),
                line(2, LineKind::Add, "zwei")
            ]
        );
    }

    #[test]
    fn content_that_looks_like_a_header_stays_content() {
        // Entfernte Zeile "-- kommentar" beginnt mit "--"; das ist keine Kopfzeile.
        let patch = "@@ -1,2 +1,1 @@\n--- kommentar\n ende";
        let z = parse_patch(patch);
        assert_eq!(z[0], line(1, LineKind::Del, "-- kommentar"));
        assert_eq!(z[1], line(1, LineKind::Context, "ende"));
    }

    #[test]
    fn no_newline_hint_and_empty_context_line() {
        let patch = "@@ -1,3 +1,3 @@\n a\n\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file";
        let z = parse_patch(patch);
        assert_eq!(
            z,
            vec![
                line(1, LineKind::Context, "a"),
                line(2, LineKind::Context, ""),
                line(3, LineKind::Del, "b"),
                line(3, LineKind::Add, "c"),
            ]
        );
    }

    #[test]
    fn umlauts_and_file_header() {
        let patch = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-größe\n+Größe";
        let z = parse_patch(patch);
        assert_eq!(
            z,
            vec![
                line(1, LineKind::Del, "größe"),
                line(1, LineKind::Add, "Größe")
            ]
        );
    }

    #[test]
    fn empty_and_broken() {
        assert!(parse_patch("").is_empty());
        assert!(parse_patch("@@ kaputt @@\n+x").is_empty());
        // Mehr Zeilen als angekündigt: der Rest wird ignoriert.
        assert_eq!(parse_patch("@@ -1 +1 @@\n+a\n+b").len(), 1);
    }
}
