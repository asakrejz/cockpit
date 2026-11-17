use super::*;

#[test]
fn set_read_delete() {
    let s = MemoryStore::new();
    assert_eq!(s.get(GITHUB).unwrap(), None);
    assert!(!s.has(GITHUB).unwrap());
    s.set(GITHUB, &Secret::new("ghp_a")).unwrap();
    s.set(JIRA, &Secret::new("j")).unwrap();
    assert_eq!(s.get(GITHUB).unwrap().unwrap().expose(), "ghp_a");
    assert!(s.has(GITHUB).unwrap());
    s.set(GITHUB, &Secret::new("ghp_b")).unwrap();
    assert_eq!(s.get(GITHUB).unwrap().unwrap().expose(), "ghp_b");
    s.delete(GITHUB).unwrap();
    s.delete(GITHUB).unwrap(); // a second delete is no error
    assert_eq!(s.get(GITHUB).unwrap(), None);
    assert!(s.has(JIRA).unwrap());
}

#[test]
fn debug_shows_no_values() {
    let s = MemoryStore::new();
    s.set(GITHUB, &Secret::new("ghp_geheim")).unwrap();
    let output = format!("{s:?} {:?}", s.get(GITHUB).unwrap());
    assert!(!output.contains("ghp_geheim"), "{output}");
    assert!(output.contains("github"));
}
