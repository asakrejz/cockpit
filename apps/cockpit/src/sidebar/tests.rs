use super::*;

/// Every page of the former sidebar is reachable: in the pill or behind "Mehr".
#[test]
fn every_former_sidebar_target_is_in_the_pill_or_in_more() {
    let labels: Vec<Key> = TARGETS.iter().map(|t| t.label).collect();
    for expected in [
        Key::NavAttention,
        Key::NavMine,
        Key::NavTeam,
        Key::NavSnoozed,
        Key::NavOverview,
        Key::NavReviews,
        Key::NavAgents,
        Key::NavLog,
        Key::NavLimits,
        Key::NavNotifications,
        Key::NavSetup,
    ] {
        assert!(labels.contains(&expected), "{expected:?} is missing");
    }
    assert_eq!(TARGETS.len(), 11);
}

#[test]
fn no_two_targets_share_a_route() {
    for (i, a) in TARGETS.iter().enumerate() {
        for b in &TARGETS[i + 1..] {
            assert_ne!((a.route)(), (b.route)(), "{:?} and {:?}", a.label, b.label);
        }
    }
}

#[test]
fn the_pill_has_four_targets_and_more_holds_the_rest() {
    let bar = TARGETS.iter().filter(|t| t.slot == Slot::Bar).count();
    let more = TARGETS.iter().filter(|t| t.slot == Slot::More).count();
    assert_eq!(bar, 4);
    assert_eq!(bar + more, TARGETS.len());
    assert!(more > 0);
}

#[test]
fn the_pill_keeps_the_four_main_targets() {
    let bar: Vec<Key> = TARGETS
        .iter()
        .filter(|t| t.slot == Slot::Bar)
        .map(|t| t.short)
        .collect();
    assert_eq!(
        bar,
        [
            Key::NavAttentionShort,
            Key::NavMineShort,
            Key::NavReviewsShort,
            Key::NavAgentsShort
        ]
    );
}

#[test]
fn the_capsule_slides_to_the_slot_of_the_current_page() {
    assert_eq!(active_slot(&Route::Inbox(Filter::All)), Some(0));
    assert_eq!(active_slot(&Route::Inbox(Filter::Mine)), Some(1));
    assert_eq!(active_slot(&Route::Review(None)), Some(2));
    assert_eq!(active_slot(&Route::Agent(Some("BAU-1".into()))), Some(3));
    // Everything behind "Mehr" lights the last slot.
    assert_eq!(active_slot(&Route::Inbox(Filter::Team)), Some(4));
    assert_eq!(active_slot(&Route::Setup), Some(4));
    assert_eq!(active_slot(&Route::Limits), Some(4));
}

#[test]
fn every_page_lights_exactly_one_slot() {
    for t in &TARGETS {
        assert!(active_slot(&(t.route)()).is_some(), "{:?}", t.label);
    }
}

#[test]
fn pull_requests_and_agents_stay_current_for_every_item() {
    let review = TARGETS.iter().find(|t| t.label == Key::NavReviews).unwrap();
    assert!(review.is_current(&Route::Review(None)));
    assert!(review.is_current(&Route::Review(Some(cockpit_core::PrKey::from(3)))));
    assert!(!review.is_current(&Route::Agent(None)));
}
