//! Pages and their addresses. The address is in the hash of the URL (`#/review/acme/app/87`),
//! so that the back button and bookmarks work.

use cockpit_core::setup::Step;
use cockpit_core::{Filter, PrKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// Set up and change connections.
    Setup,
    /// A step of the setup assistant (`#/setup/jira`). The page is the same as for `Setup`.
    SetupStep(Step),
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
    /// The route as far as it decides which page is shown: all addresses of the setup assistant
    /// are one page, so moving between its steps never rebuilds the page.
    pub fn page(&self) -> Route {
        match self {
            Route::SetupStep(_) => Route::Setup,
            other => other.clone(),
        }
    }

    pub fn is_setup(&self) -> bool {
        matches!(self, Route::Setup | Route::SetupStep(_))
    }

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
            (Some("setup" | "einrichten"), step) => match step.and_then(Step::from_id) {
                Some(step) => Route::SetupStep(step),
                None => Route::Setup,
            },
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
            Route::SetupStep(step) => step.hash(),
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
mod tests;
