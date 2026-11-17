use super::*;

#[test]
fn every_route_survives_a_roundtrip() {
    for route in [
        Route::Setup,
        Route::SetupStep(Step::Container),
        Route::SetupStep(Step::Jira),
        Route::SetupStep(Step::GitHub),
        Route::SetupStep(Step::Model),
        Route::SetupStep(Step::Live),
        Route::SetupStep(Step::Done),
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

#[test]
fn setup_addresses_with_and_without_a_step() {
    assert_eq!(Route::parse("#/setup"), Route::Setup);
    assert_eq!(Route::parse("#/einrichten"), Route::Setup);
    assert_eq!(Route::parse("#/setup/"), Route::Setup);
    assert_eq!(Route::parse("#/setup/jira"), Route::SetupStep(Step::Jira));
    assert_eq!(
        Route::parse("#/einrichten/github"),
        Route::SetupStep(Step::GitHub)
    );
    // The older German step ids and unknown steps.
    assert_eq!(
        Route::parse("#/setup/modell"),
        Route::SetupStep(Step::Model)
    );
    assert_eq!(Route::parse("#/setup/fertig"), Route::SetupStep(Step::Done));
    assert_eq!(Route::parse("#/setup/unsinn"), Route::Setup);
    assert_eq!(Route::SetupStep(Step::Jira).hash(), "#/setup/jira");
}

#[test]
fn all_setup_steps_are_one_page() {
    assert_eq!(Route::SetupStep(Step::Live).page(), Route::Setup);
    assert_eq!(Route::Setup.page(), Route::Setup);
    assert_eq!(Route::Log.page(), Route::Log);
    assert!(Route::SetupStep(Step::Done).is_setup() && Route::Setup.is_setup());
    assert!(!Route::Log.is_setup());
}
