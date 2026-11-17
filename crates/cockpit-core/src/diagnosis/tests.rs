use super::*;
use crate::demo;

fn review(number: u32) -> Review {
    demo::workspace().review(number).unwrap().clone()
}

fn check<'a>(review: &'a Review, name: &str) -> &'a CheckRun {
    review.check_runs.iter().find(|c| c.name == name).unwrap()
}

#[test]
fn the_demo_lint_error_is_found_with_file_line_and_fix() {
    let r = review(85);
    let d = diagnose(&r, check(&r, "Lint"));
    assert_eq!(d.file.as_deref(), Some("list.rs"));
    assert_eq!(d.line, Some(62));
    assert_eq!(d.location().as_deref(), Some("list.rs · Zeile 62"));
    assert!(d.cause.contains("`inset`"), "{}", d.cause);
    assert!(d.changes_code());
    let fix = d.fix.as_ref().unwrap();
    assert_eq!(fix.after, None);
    assert_eq!(fix.before, "        let inset = icon.map(|_| 46);");
    // The diff shows the line as removed, with surroundings.
    let patch = d.patch.unwrap();
    assert_eq!(patch.name, "list.rs");
    let kinds: Vec<LineKind> = patch.lines.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, [LineKind::Context, LineKind::Del, LineKind::Context]);
}

#[test]
fn a_browser_timeout_is_flaky_and_needs_no_code_change() {
    let r = review(85);
    let d = diagnose(&r, check(&r, "Tests (Browser)"));
    assert!(d.cause.contains("Zeitüberschreitung"), "{}", d.cause);
    assert!(!d.changes_code());
    assert!(d.patch.is_none() && d.fix.is_none());
    assert!(d.remedy.contains("neu starten"));
}

#[test]
fn a_real_test_failure_gets_no_automatic_fix() {
    let r = review(85);
    let mut c = check(&r, "Tests (Browser)").clone();
    c.log = "running 1 test\ntest a::b ... FAILED\nthread panicked: left != right\n".into();
    let d = diagnose(&r, &c);
    assert!(d.cause.contains("`a::b`") && d.cause.contains("left != right"));
    assert!(!d.changes_code());
}

#[test]
fn an_unused_line_that_the_pull_request_does_not_have_is_not_touched() {
    let r = review(85);
    let mut c = check(&r, "Lint").clone();
    c.log = c.log.replace("62 |", "99 |").replace(":62:13", ":99:13");
    let d = diagnose(&r, &c);
    assert_eq!(d.line, Some(99));
    assert!(!d.changes_code(), "no fix without a matching line");
}

#[test]
fn an_empty_or_unknown_log_still_gets_an_honest_answer() {
    let r = review(85);
    let mut c = check(&r, "Lint").clone();
    c.log = String::new();
    assert!(diagnose(&r, &c).cause.contains("leer"));
    c.log = "$ make\nBoom\n".into();
    assert!(diagnose(&r, &c).cause.contains("Boom"));
}

#[test]
fn applying_the_fix_removes_the_line_and_moves_later_comments() {
    let mut r = review(85);
    let d = diagnose(&r, check(&r, "Lint"));
    let fix = d.fix.unwrap();
    // Comment below the line: moves up.
    let id = r.add_line_comment("list.rs", 2, "Passt.", None).unwrap();
    assert!(r.apply_fix(&fix));
    let file = r.files.iter().find(|f| f.name == "list.rs").unwrap();
    assert!(file.lines.iter().all(|l| l.text != fix.before));
    assert_eq!(file.lines.len(), 2);
    assert_eq!(r.line_comments.iter().find(|c| c.id == id).unwrap().line, 1);
    // A second time the line no longer exists.
    assert!(!r.apply_fix(&fix));
}
