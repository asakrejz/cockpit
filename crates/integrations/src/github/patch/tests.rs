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
    let lines = parse_patch(patch);
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[1], line(2, LineKind::Add, "neu"));
    assert_eq!(lines[3], line(20, LineKind::Del, "alt"));
    assert_eq!(lines[4], line(21, LineKind::Add, "neu2"));

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
    let lines = parse_patch(patch);
    assert_eq!(lines[0], line(1, LineKind::Del, "-- kommentar"));
    assert_eq!(lines[1], line(1, LineKind::Context, "ende"));
}

#[test]
fn no_newline_hint_and_empty_context_line() {
    let patch =
        "@@ -1,3 +1,3 @@\n a\n\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file";
    let lines = parse_patch(patch);
    assert_eq!(
        lines,
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
    let lines = parse_patch(patch);
    assert_eq!(
        lines,
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
