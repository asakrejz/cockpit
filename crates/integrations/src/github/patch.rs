//! Parser for unified-diff patches, as GitHub delivers them in the `patch` field.

use cockpit_core::{DiffLine, LineKind};

/// `@@ -a,b +c,d @@ …` → `(a, b, c, d)`; missing lengths mean 1.
fn hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let again = rest.split(" @@").next()?;
    let part = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (a, b) = part(old)?;
    let (c, d) = part(again)?;
    Some((a, b, c, d))
}

/// Turns a patch into diff lines. Line numbers as in `cockpit-core`: added and
/// unchanged lines count in the new state, removed ones in the old.
///
/// The `@@` headers themselves are not taken over as lines; between two sections
/// the number jumps accordingly. Lines outside a section (file header,
/// „\ No newline at end of file“) are ignored.
pub fn parse_patch(patch: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let (mut old, mut again) = (0u32, 0u32);
    let (mut old_rest, mut again_rest) = (0u32, 0u32);

    for line in patch.lines() {
        if line.starts_with("@@") {
            if let Some((a, b, c, d)) = hunk_header(line) {
                (old, old_rest, again, again_rest) = (a, b, c, d);
            } else {
                // Broken header: better to show nothing than wrong numbers.
                (old_rest, again_rest) = (0, 0);
            }
            continue;
        }
        if line.starts_with('\\') || (old_rest == 0 && again_rest == 0) {
            continue;
        }
        // An empty line is a context line whose space got lost.
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
            "-" if old_rest > 0 => {
                out.push(DiffLine {
                    no: old,
                    kind: LineKind::Del,
                    text: text.into(),
                });
                old += 1;
                old_rest -= 1;
            }
            " " | "" if old_rest > 0 && again_rest > 0 => {
                out.push(DiffLine {
                    no: again,
                    kind: LineKind::Context,
                    text: text.into(),
                });
                old += 1;
                again += 1;
                old_rest -= 1;
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
        // Removed line "-- kommentar" starts with "--"; that is not a header line.
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
        // More lines than announced: the rest is ignored.
        assert_eq!(parse_patch("@@ -1 +1 @@\n+a\n+b").len(), 1);
    }
}
