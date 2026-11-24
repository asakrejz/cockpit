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
        Route::InboxItem(Filter::All, "bau142".into()),
        Route::InboxItem(Filter::Mine, "gh:acme/app#12".into()),
        Route::InboxItem(Filter::Snoozed, "ä b".into()),
        Route::Overview,
        Route::OverviewMember("Ada Lovelace".into()),
        Route::OverviewAll,
        Route::OverviewMember("@all".into()),
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
fn unknown_addresses_lead_to_the_inbox() {
    for hash in [
        "",
        "#",
        "#/",
        "#/does-not-exist",
        "unsinn",
        "#/einrichten",
        "#/meine",
        "#/uebersicht",
    ] {
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
    // A pull request without a known repo is addressed by its number.
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
    assert_eq!(Route::parse("#/setup/"), Route::Setup);
    assert_eq!(Route::parse("#/setup/jira"), Route::SetupStep(Step::Jira));
    assert_eq!(
        Route::parse("#/setup/model"),
        Route::SetupStep(Step::Model)
    );
    assert_eq!(Route::parse("#/setup/done"), Route::SetupStep(Step::Done));
    // Unknown steps, among them the German ids.
    assert_eq!(Route::parse("#/setup/unsinn"), Route::Setup);
    assert_eq!(Route::parse("#/setup/modell"), Route::Setup);
    assert_eq!(Route::parse("#/setup/fertig"), Route::Setup);
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

#[test]
fn only_an_entry_is_the_detail_level() {
    for (route, level) in [
        (Route::Inbox(Filter::All), StackLevel::List),
        (
            Route::InboxItem(Filter::All, "a".into()),
            StackLevel::Detail,
        ),
        (Route::Overview, StackLevel::List),
        (Route::OverviewMember("Ada".into()), StackLevel::Detail),
        (Route::OverviewAll, StackLevel::Detail),
        (Route::Review(None), StackLevel::List),
        (Route::Review(Some(PrKey::from(87))), StackLevel::Detail),
        (Route::Agent(None), StackLevel::List),
        (Route::Agent(Some("BAU-142".into())), StackLevel::Detail),
        (Route::Log, StackLevel::List),
        (Route::Setup, StackLevel::List),
    ] {
        assert_eq!(route.level(), level, "{route:?}");
    }
}

#[test]
fn list_and_detail_are_one_page() {
    let list = Route::Inbox(Filter::Mine);
    let detail = Route::InboxItem(Filter::Mine, "a".into());
    assert_eq!(detail.page(), list.page());
    assert_eq!(
        Route::OverviewMember("Ada".into()).page(),
        Route::Overview.page()
    );
    assert_eq!(Route::OverviewAll.page(), Route::Overview.page());
    // Another filter is another page.
    assert_ne!(
        Route::InboxItem(Filter::Team, "a".into()).page(),
        list.page()
    );
}

#[test]
fn item_addresses_read_like_the_list_plus_the_entry() {
    assert_eq!(
        Route::InboxItem(Filter::All, "bau142".into()).hash(),
        "#/item/bau142"
    );
    assert_eq!(
        Route::InboxItem(Filter::Mine, "bau142".into()).hash(),
        "#/mine/item/bau142"
    );
    assert_eq!(Route::parse("#/mine/item"), Route::Inbox(Filter::Mine));
}

#[test]
fn go_to_entry_opens_exactly_that_entry_in_the_unfiltered_list() {
    // The address carries the ID: the inbox chooses that entry, and on a narrow screen the
    // detail level opens.
    let route = Route::to_entry("bau142");
    assert_eq!(route, Route::InboxItem(Filter::All, "bau142".into()));
    assert_eq!(route.hash(), "#/item/bau142");
    assert_eq!(route.level(), StackLevel::Detail);
    assert_eq!(Route::parse(&route.hash()), route);
    // IDs with characters of an address survive.
    for id in ["gh-acme-app-87", "gh:acme/app#12", "ä b"] {
        assert_eq!(
            Route::parse(&Route::to_entry(id).hash()),
            Route::to_entry(id),
            "{id}"
        );
    }
}

#[test]
fn everybody_in_the_overview_has_an_address_a_person_never_has() {
    assert_eq!(Route::OverviewAll.hash(), "#/overview/@all");
    // A person called like the marker is still a person: the name is written escaped.
    assert_eq!(
        Route::OverviewMember("@all".into()).hash(),
        "#/overview/%40all"
    );
    assert_eq!(
        Route::parse("#/overview/Ada"),
        Route::OverviewMember("Ada".into())
    );
    assert_eq!(Route::parse("#/overview"), Route::Overview);
}

#[test]
fn the_tasks_address_leads_to_the_remembered_filter() {
    for last in [Filter::All, Filter::Mine, Filter::Team, Filter::Snoozed] {
        assert_eq!(Route::resolve("", last), Route::Inbox(last));
        assert_eq!(Route::resolve("#", last), Route::Inbox(last));
        assert_eq!(Route::resolve("#/tasks", last), Route::Inbox(last));
        assert_eq!(
            Route::resolve("#/tasks/item/bau142", last),
            Route::InboxItem(last, "bau142".into())
        );
    }
}

#[test]
fn old_inbox_addresses_keep_their_filter() {
    // The remembered filter never overrides an address that names one.
    assert_eq!(
        Route::resolve("#/", Filter::Team),
        Route::Inbox(Filter::All)
    );
    assert_eq!(
        Route::resolve("#/item/bau142", Filter::Team),
        Route::InboxItem(Filter::All, "bau142".into())
    );
    assert_eq!(
        Route::resolve("#/mine/item/bau142", Filter::Team),
        Route::InboxItem(Filter::Mine, "bau142".into())
    );
    assert_eq!(
        Route::resolve("#/team", Filter::Mine),
        Route::Inbox(Filter::Team)
    );
    assert_eq!(Route::resolve("#/log", Filter::Mine), Route::Log);
}

#[test]
fn only_filterless_addresses_are_aliases() {
    for hash in ["", "#", "#/tasks", "#/tasks/item/a"] {
        assert!(Route::is_tasks_alias(hash), "{hash}");
    }
    for hash in ["#/", "#/mine", "#/item/a", "#/log", "#/nope"] {
        assert!(!Route::is_tasks_alias(hash), "{hash}");
    }
}
