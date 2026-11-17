use super::*;
use crate::review::LineKind::{Add, Context};

fn file(text: &str) -> FileDiff {
    FileDiff::new("a.css", &[(1, Context, "x"), (2, Add, text)])
}

#[test]
fn a_viewed_file_stays_viewed_until_its_diff_changes() {
    let mut v = Viewed::default();
    let f = file("a: 1;");
    assert!(!v.is_viewed(&f));
    v.set(&f, true);
    assert!(v.is_viewed(&f));
    assert!(!v.is_viewed(&file("a: 2;")), "a new push opens it again");
}

#[test]
fn unchecking_removes_the_mark() {
    let mut v = Viewed::default();
    let f = file("a: 1;");
    v.set(&f, true);
    v.set(&f, false);
    assert!(!v.is_viewed(&f));
    assert_eq!(v.count(&[f]), 0);
}

#[test]
fn it_survives_a_round_trip_through_json() {
    let mut v = Viewed::default();
    let f = file("a: 1;");
    v.set(&f, true);
    assert_eq!(Viewed::parse(&v.to_json()), v);
    assert_eq!(v.count(&[f, file("other")]), 1);
}

#[test]
fn garbage_means_nothing_viewed() {
    assert_eq!(Viewed::parse("not json"), Viewed::default());
    assert_eq!(Viewed::parse(""), Viewed::default());
}

#[test]
fn the_fingerprint_is_stable() {
    // A change of this value would reset every viewed mark – only change it on purpose.
    assert_eq!(file("a: 1;").fingerprint(), file("a: 1;").fingerprint());
    assert_ne!(file("a: 1;").fingerprint(), file("a: 2;").fingerprint());
}
