use super::*;

#[test]
fn every_route_survives_a_roundtrip() {
    for route in [
        Route::Setup,
        Route::Inbox(Filter::All),
        Route::Inbox(Filter::Mine),
        Route::Inbox(Filter::Team),
        Route::Inbox(Filter::Snoozed),
        Route::Overview,
        Route::Log,
        Route::Notifications,
        Route::Limits,
        Route::Review(None),
        Route::Review(Some(PrKey::from(87))),
        Route::Review(Some(PrKey::new("acme/app", 87))),
        Route::Agent(None),
        Route::Agent(Some("BAU-142".into())),
    ] {
        assert_eq!(Route::parse(&route.hash()), route);
    }
}

#[test]
fn older_german_addresses_still_work() {
    for (hash, route) in [
        ("#/einrichten", Route::Setup),
        ("#/meine", Route::Inbox(Filter::Mine)),
        ("#/zurueckgestellt", Route::Inbox(Filter::Snoozed)),
        ("#/uebersicht", Route::Overview),
        ("#/protokoll", Route::Log),
        ("#/benachrichtigungen", Route::Notifications),
        ("#/grenzen", Route::Limits),
    ] {
        assert_eq!(Route::parse(hash), route, "{hash}");
    }
}

#[test]
fn unknown_addresses_lead_to_the_inbox() {
    for hash in ["", "#", "#/", "#/does-not-exist", "unsinn"] {
        assert_eq!(Route::parse(hash), Route::Inbox(Filter::All), "{hash}");
    }
}

#[test]
fn equal_numbers_of_two_repos_have_different_addresses() {
    let (a, b) = (PrKey::new("acme/app", 87), PrKey::new("acme/web", 87));
    assert_ne!(
        Route::Review(Some(a.clone())).hash(),
        Route::Review(Some(b)).hash()
    );
    assert_eq!(Route::Review(Some(a)).hash(), "#/review/acme/app/87");
}

#[test]
fn broken_parameters_are_dropped() {
    assert_eq!(Route::parse("#/review/abc"), Route::Review(None));
    assert_eq!(Route::parse("#/agent/"), Route::Agent(None));
    assert_eq!(
        Route::parse("#review/87"),
        Route::Review(Some(PrKey::from(87)))
    );
    assert_eq!(
        Route::parse("#/review/x/87"),
        Route::Review(None),
        "not owner/name"
    );
}
