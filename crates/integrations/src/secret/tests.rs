use super::*;

#[test]
fn debug_never_shows_the_key() {
    let s = Secret::new("ghp_geheim");
    assert_eq!(format!("{s:?}"), "Secret(***)");
    assert_eq!(format!("{:#?}", Some(&s)), "Some(\n    Secret(***),\n)");
    assert_eq!(s.expose(), "ghp_geheim");
}
