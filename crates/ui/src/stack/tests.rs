use super::*;

#[test]
fn levels_have_distinct_attributes() {
    assert_eq!(StackLevel::List.attr(), "list");
    assert_eq!(StackLevel::Detail.attr(), "detail");
    assert_eq!(StackLevel::default(), StackLevel::List);
}

#[test]
fn motion_follows_the_direction_of_the_change() {
    use StackLevel::*;
    assert_eq!(List.motion_to(Detail), Some("forward"));
    assert_eq!(Detail.motion_to(List), Some("back"));
    assert_eq!(List.motion_to(List), None);
    assert_eq!(Detail.motion_to(Detail), None);
}

#[test]
fn back_swipe_needs_the_edge_distance_and_a_flat_path() {
    // From the edge, far enough, flat: back.
    assert!(is_back_swipe(10.0, 300.0, 150.0, 310.0));
    // Starts in the middle of the screen: that is a normal scroll or drag.
    assert!(!is_back_swipe(120.0, 300.0, 300.0, 300.0));
    // Too short.
    assert!(!is_back_swipe(10.0, 300.0, 50.0, 300.0));
    // Mostly vertical.
    assert!(!is_back_swipe(10.0, 300.0, 100.0, 450.0));
    // Leftwards.
    assert!(!is_back_swipe(10.0, 300.0, -50.0, 300.0));
}
