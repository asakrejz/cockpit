use super::*;

#[test]
fn plain_ids_stay_readable() {
    assert_eq!(encode("bau142"), "bau142");
    assert_eq!(encode("BAU-142"), "BAU-142");
}

#[test]
fn special_characters_never_break_the_address() {
    let id = "gh:acme/app#12 ü";
    let text = encode(id);
    assert!(!text.contains(['/', '#', ' ', ':']));
    assert_eq!(decode(&text), id);
}

#[test]
fn a_broken_escape_is_kept() {
    assert_eq!(decode("a%zzb"), "a%zzb");
    assert_eq!(decode("100%"), "100%");
}
