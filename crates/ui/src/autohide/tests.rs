use super::*;

#[test]
fn scrolling_down_hides_and_up_shows() {
    assert!(next_hidden(false, 20.0, 200.0, false));
    assert!(!next_hidden(true, -10.0, 190.0, false));
}

#[test]
fn small_moves_keep_the_state() {
    assert!(next_hidden(true, 1.0, 200.0, false));
    assert!(!next_hidden(false, -1.0, 200.0, false));
}

#[test]
fn top_and_end_always_show() {
    assert!(!next_hidden(true, 30.0, 0.0, false));
    assert!(!next_hidden(true, 30.0, 500.0, true));
}

#[test]
fn the_top_zone_never_hides() {
    assert!(!next_hidden(false, 20.0, 40.0, false));
}

#[test]
fn keyboard_needs_a_narrow_screen_and_a_big_loss() {
    assert!(keyboard_open(400.0, 800.0, 390.0));
    assert!(!keyboard_open(700.0, 800.0, 390.0));
    assert!(!keyboard_open(400.0, 800.0, 1280.0));
}
