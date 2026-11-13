//! Pages and their addresses. The address is in the hash of the URL (`#/review/acme/app/87`),
//! so that the back button and bookmarks work.

use cockpit_core::{Filter, PrKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// Set up and change connections.
    Setup,
    /// The inbox with a filter.
    Inbox(Filter),
    /// Team overview: who works on what and what is stuck.
    Overview,
    /// Log of all changes.
    Log,
    /// Settings for notifications.
    Notifications,
    /// Costs, limits and rules per repo for the agents.
    Limits,
    /// A review of a pull request (repo and number). Without one: the first open review.
    /// The older address with only a number (`#/review/87`) still works.
    Review(Option<PrKey>),
    /// An agent run. Without a ticket: a sensible suggestion.
    Agent(Option<String>),
}

impl Route {
    pub fn parse(hash: &str) -> Route {
        let mut parts = hash
            .trim_start_matches('#')
            .split('/')
            .filter(|p| !p.is_empty());
        let (first, second) = (parts.next(), parts.next());
        // What follows `review/`: `acme/app/87` or `87`.
        let rest: Vec<&str> = std::iter::once(second).flatten().chain(parts).collect();
        match (first, second) {
            // The older German addresses keep working (bookmarks).
            (Some("setup" | "einrichten"), _) => Route::Setup,
            (Some("mine" | "meine"), _) => Route::Inbox(Filter::Mine),
            (Some("team"), _) => Route::Inbox(Filter::Team),
            (Some("snoozed" | "zurueckgestellt"), _) => Route::Inbox(Filter::Snoozed),
            (Some("overview" | "uebersicht"), _) => Route::Overview,
            (Some("log" | "protokoll"), _) => Route::Log,
            (Some("notifications" | "benachrichtigungen"), _) => Route::Notifications,
            (Some("limits" | "grenzen"), _) => Route::Limits,
            (Some("review"), _) => Route::Review(PrKey::from_route(&rest.join("/"))),
            (Some("agent"), key) => Route::Agent(key.map(str::to_string)),
            _ => Route::Inbox(Filter::All),
        }
    }

    pub fn hash(&self) -> String {
        match self {
            Route::Setup => "#/setup".into(),
            Route::Inbox(Filter::All) => "#/".into(),
            Route::Inbox(Filter::Mine) => "#/mine".into(),
            Route::Inbox(Filter::Team) => "#/team".into(),
            Route::Inbox(Filter::Snoozed) => "#/snoozed".into(),
            Route::Overview => "#/overview".into(),
            Route::Log => "#/log".into(),
            Route::Notifications => "#/notifications".into(),
            Route::Limits => "#/limits".into(),
            Route::Review(None) => "#/review".into(),
            Route::Review(Some(key)) => format!("#/review/{}", key.route()),
            Route::Agent(None) => "#/agent".into(),
            Route::Agent(Some(key)) => format!("#/agent/{key}"),
        }
    }
}

#[cfg(test)]
mod tests {
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
}
