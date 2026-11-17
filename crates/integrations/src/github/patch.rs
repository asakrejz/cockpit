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
mod tests;
