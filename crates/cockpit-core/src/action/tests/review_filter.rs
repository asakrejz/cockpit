//! The filter of the Reviews tab: applying, persisting and resetting.

use super::*;
use crate::ReviewFilter;

fn narrowed() -> ReviewFilter {
    ReviewFilter {
        repos: vec!["acme/shop".into()],
        ..Default::default()
    }
}

#[test]
fn applying_the_review_filter_is_stored_and_logged() {
    let mut w = ws();
    assert!(w.review_filter.is_default());
    ok(&mut w, Action::SetReviewFilter { filter: narrowed() }, 1);
    assert_eq!(w.review_filter, narrowed());
    assert_eq!(w.log.last().unwrap().text, "Filter angewendet");
    fails(
        &mut w,
        Action::SetReviewFilter { filter: narrowed() },
        ActionError::Unchanged,
    );
    ok(
        &mut w,
        Action::SetReviewFilter {
            filter: ReviewFilter::default(),
        },
        1,
    );
    assert!(w.review_filter.is_default());
    assert_eq!(w.log.last().unwrap().text, "Filter zurückgesetzt");
}

#[test]
fn the_choice_survives_a_reset_of_the_demo_data() {
    let mut w = ws();
    ok(&mut w, Action::SetReviewFilter { filter: narrowed() }, 1);
    let _ = w.apply(Action::ResetDemo, Actor::User, T);
    assert_eq!(w.review_filter, narrowed());
}
