//! Pages and their addresses. The address is in the hash of the URL (`#/review/acme/app/87`),
//! so that the back button and bookmarks work.

use cockpit_core::setup::Step;
use cockpit_core::{Filter, PrKey};
use feature_ui::StackLevel;

mod segment;

/// The piece of the address that chooses everybody in the overview. A name never starts with
/// `@` in the address: `@` is written as `%40` there.
const ALL_PEOPLE: &str = "@all";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// Set up and change connections.
    Setup,
    /// A step of the setup assistant (`#/setup/jira`). The page is the same as for `Setup`.
    SetupStep(Step),
    /// The inbox with a filter.
    Inbox(Filter),
    /// One entry of the inbox (`#/mine/item/bau142`): the detail level on a narrow screen.
    InboxItem(Filter, String),
    /// Team overview: who works on what and what is stuck.
    Overview,
    /// The overview with everybody chosen (`#/overview/@all`): the detail level on a narrow
    /// screen. Without a choice (`Overview`) the wide screen shows everybody too.
    OverviewAll,
    /// The overview with one person chosen (`#/overview/Ada%20L`): the detail level on a
    /// narrow screen.
    OverviewMember(String),
    /// Log of all changes.
    Log,
    /// Settings for notifications.
    Notifications,
    /// Costs, limits and rules per repo for the agents.
    Limits,
    /// A review of a pull request (repo and number). Without one: the first open review. A pull
    /// request whose repo is not known is addressed by its number alone (`#/review/87`).
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
            // Going from the list to the detail keeps the page, so it can slide.
            Route::InboxItem(filter, _) => Route::Inbox(*filter),
            Route::OverviewMember(_) | Route::OverviewAll => Route::Overview,
            other => other.clone(),
        }
    }

    /// The level of the page a narrow screen shows: the list, or one entry of it. Wide screens
    /// show every level at once.
    pub fn level(&self) -> StackLevel {
        match self {
            Route::InboxItem(..)
            | Route::OverviewMember(_)
            | Route::OverviewAll
            | Route::Review(Some(_))
            | Route::Agent(Some(_)) => StackLevel::Detail,
            _ => StackLevel::List,
        }
    }

    /// The address that opens exactly this entry of the inbox (`#/item/bau142`): the unfiltered
    /// list with the entry chosen.
    pub fn to_entry(id: &str) -> Route {
        Route::InboxItem(Filter::All, id.to_string())
    }

    /// The address of the navigation entry "Tasks": it leads to the filter chosen last.
    pub const TASKS_HASH: &'static str = "#/tasks";

    /// Is this an address that only says "the tasks page" (no bare `#/`, which is the filter
    /// "Alle")? Such an address is replaced by the one of the remembered filter.
    pub fn is_tasks_alias(hash: &str) -> bool {
        let path = hash.trim_start_matches('#');
        path.is_empty() || path.trim_matches('/').split('/').next() == Some("tasks")
    }

    /// Like [`Route::parse`], but the addresses without a filter (no address at all, `#/tasks`,
    /// `#/tasks/item/<id>`) land on the filter chosen last (`last`; "Ich" by default).
    pub fn resolve(hash: &str, last: Filter) -> Route {
        if !Route::is_tasks_alias(hash) {
            return Route::parse(hash);
        }
        let parts: Vec<&str> = hash
            .trim_start_matches('#')
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();
        match (parts.get(1), parts.get(2)) {
            (Some(&"item"), Some(id)) => Route::InboxItem(last, segment::decode(id)),
            _ => Route::Inbox(last),
        }
    }

    pub fn is_setup(&self) -> bool {
        matches!(self, Route::Setup | Route::SetupStep(_))
    }

    pub fn parse(hash: &str) -> Route {
        let parts: Vec<&str> = hash
            .trim_start_matches('#')
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();
        let (first, second) = (parts.first().copied(), parts.get(1).copied());
        // What follows `review/`: `acme/app/87` or `87`.
        let rest: Vec<&str> = parts.iter().skip(1).copied().collect();
        // `item/<id>` after the filter (`#/mine/item/bau142`, `#/item/bau142`).
        let item = |filter: Filter, at: usize| match (parts.get(at), parts.get(at + 1)) {
            (Some(&"item"), Some(id)) => Route::InboxItem(filter, segment::decode(id)),
            _ => Route::Inbox(filter),
        };
        match (first, second) {
            (Some("setup"), step) => match step.and_then(Step::from_id) {
                Some(step) => Route::SetupStep(step),
                None => Route::Setup,
            },
            (Some("mine"), _) => item(Filter::Mine, 1),
            (Some("team"), _) => item(Filter::Team, 1),
            (Some("snoozed"), _) => item(Filter::Snoozed, 1),
            (Some("item"), _) => item(Filter::All, 0),
            (Some("overview"), Some(ALL_PEOPLE)) => Route::OverviewAll,
            (Some("overview"), Some(name)) => Route::OverviewMember(segment::decode(name)),
            (Some("overview"), None) => Route::Overview,
            (Some("log"), _) => Route::Log,
            (Some("notifications"), _) => Route::Notifications,
            (Some("limits"), _) => Route::Limits,
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
            Route::InboxItem(filter, id) => {
                let list = Route::Inbox(*filter).hash();
                format!(
                    "{}/item/{}",
                    list.trim_end_matches('/'),
                    segment::encode(id)
                )
            }
            Route::Overview => "#/overview".into(),
            Route::OverviewAll => format!("#/overview/{ALL_PEOPLE}"),
            Route::OverviewMember(name) => format!("#/overview/{}", segment::encode(name)),
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
