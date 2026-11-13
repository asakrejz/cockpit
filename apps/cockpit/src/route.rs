//! Seiten und ihre Adressen. Die Adresse steht im Hash der URL (`#/review/87`),
//! damit Zurück-Button und Lesezeichen funktionieren.

use cockpit_core::Filter;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// Verbindungen einrichten und ändern.
    Setup,
    /// Die Inbox mit einem Filter.
    Inbox(Filter),
    /// Team-Übersicht: wer woran arbeitet und was hängt.
    Overview,
    /// Protokoll aller Änderungen.
    Log,
    /// Einstellungen für Benachrichtigungen.
    Notifications,
    /// Kosten, Grenzen und Regeln pro Repo für die Agenten.
    Limits,
    /// Ein Review. Ohne Nummer: das erste offene.
    Review(Option<u32>),
    /// Ein Agenten-Lauf. Ohne Ticket: ein sinnvoller Vorschlag.
    Agent(Option<String>),
}

impl Route {
    pub fn parse(hash: &str) -> Route {
        let mut parts = hash
            .trim_start_matches('#')
            .split('/')
            .filter(|p| !p.is_empty());
        match (parts.next(), parts.next()) {
            // The older German addresses keep working (bookmarks).
            (Some("setup" | "einrichten"), _) => Route::Setup,
            (Some("mine" | "meine"), _) => Route::Inbox(Filter::Mine),
            (Some("team"), _) => Route::Inbox(Filter::Team),
            (Some("snoozed" | "zurueckgestellt"), _) => Route::Inbox(Filter::Snoozed),
            (Some("overview" | "uebersicht"), _) => Route::Overview,
            (Some("log" | "protokoll"), _) => Route::Log,
            (Some("notifications" | "benachrichtigungen"), _) => Route::Notifications,
            (Some("limits" | "grenzen"), _) => Route::Limits,
            (Some("review"), number) => Route::Review(number.and_then(|n| n.parse().ok())),
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
            Route::Review(Some(number)) => format!("#/review/{number}"),
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
            Route::Review(Some(87)),
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
    fn broken_parameters_are_dropped() {
        assert_eq!(Route::parse("#/review/abc"), Route::Review(None));
        assert_eq!(Route::parse("#/agent/"), Route::Agent(None));
        assert_eq!(Route::parse("#review/87"), Route::Review(Some(87)));
    }
}
