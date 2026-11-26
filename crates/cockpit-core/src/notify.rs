//! Notifications: which events are worth mentioning, and where do they go?
//!
//! * [`events`] compares the workspace before and after an action and derives the
//!   [`Event`]s: review request, mention, agent waiting, agent done, checks red,
//!   ticket assigned.
//! * [`NotifySettings`] holds the channels ([`Channel`]) per reason and the quiet hours.
//!   The settings live in the [`Workspace`](crate::Workspace) and change only through
//!   actions.
//! * [`deliveries`] says which event is reported over which channel.

use cockpit_i18n::{Msg, msg};
use serde::{Deserialize, Serialize};

use crate::agent::AgentRun;
use crate::model::{Checks, Item, Person, PrRef, ReasonKind, Scope};
use crate::workspace::Workspace;

/// Minutes of a day.
const DAY: u16 = 24 * 60;

/// What one can be notified about.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotifyReason {
    ReviewRequested,
    Mention,
    AgentWaiting,
    AgentFinished,
    ChecksFailed,
    Assigned,
    /// A limit (budget, time limit) has stopped a run.
    AgentStopped,
}

impl NotifyReason {
    pub const ALL: [NotifyReason; 7] = [
        NotifyReason::ReviewRequested,
        NotifyReason::Mention,
        NotifyReason::AgentWaiting,
        NotifyReason::AgentFinished,
        NotifyReason::ChecksFailed,
        NotifyReason::Assigned,
        NotifyReason::AgentStopped,
    ];

    pub fn id(self) -> &'static str {
        match self {
            NotifyReason::ReviewRequested => "review",
            NotifyReason::Mention => "erwaehnung",
            NotifyReason::AgentWaiting => "agent-wartet",
            NotifyReason::AgentFinished => "agent-fertig",
            NotifyReason::ChecksFailed => "checks",
            NotifyReason::Assigned => "zugewiesen",
            NotifyReason::AgentStopped => "agent-angehalten",
        }
    }

    pub fn from_id(id: &str) -> Option<NotifyReason> {
        Self::ALL.into_iter().find(|r| r.id() == id)
    }

    pub fn msg(self) -> Msg {
        match self {
            NotifyReason::ReviewRequested => msg!(CoreNotifyReasonReview),
            NotifyReason::Mention => msg!(CoreNotifyReasonMention),
            NotifyReason::AgentWaiting => msg!(CoreNotifyReasonWaiting),
            NotifyReason::AgentFinished => msg!(CoreNotifyReasonFinished),
            NotifyReason::ChecksFailed => msg!(CoreNotifyReasonChecks),
            NotifyReason::Assigned => msg!(CoreNotifyReasonAssigned),
            NotifyReason::AgentStopped => msg!(CoreNotifyReasonStopped),
        }
    }

    /// A sentence on when it reports.
    pub fn hint_msg(self) -> Msg {
        match self {
            NotifyReason::ReviewRequested => msg!(CoreNotifyReasonReviewHint),
            NotifyReason::Mention => msg!(CoreNotifyReasonMentionHint),
            NotifyReason::AgentWaiting => msg!(CoreNotifyReasonWaitingHint),
            NotifyReason::AgentFinished => msg!(CoreNotifyReasonFinishedHint),
            NotifyReason::ChecksFailed => msg!(CoreNotifyReasonChecksHint),
            NotifyReason::Assigned => msg!(CoreNotifyReasonAssignedHint),
            NotifyReason::AgentStopped => msg!(CoreNotifyReasonStoppedHint),
        }
    }
}

/// Where a notification can go.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// System notification of the browser (Notification API) and hint in the app.
    Browser,
}

impl Channel {
    pub const ALL: [Channel; 1] = [Channel::Browser];

    pub fn id(self) -> &'static str {
        match self {
            Channel::Browser => "browser",
        }
    }

    pub fn from_id(id: &str) -> Option<Channel> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    pub fn msg(self) -> Msg {
        match self {
            Channel::Browser => msg!(CoreNotifyChannelBrowser),
        }
    }
}

/// Which channels are on for a reason. Other channels in a stored state (email, Slack) are ignored
/// when it is read.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Channels {
    #[serde(default)]
    pub browser: bool,
}

impl Channels {
    pub fn get(self, channel: Channel) -> bool {
        match channel {
            Channel::Browser => self.browser,
        }
    }

    fn set(&mut self, channel: Channel, on: bool) {
        match channel {
            Channel::Browser => self.browser = on,
        }
    }
}

/// Quiet hours: during this time the browser reports nothing. Values in minutes since midnight
/// (local time). If `to` is before `from`, the quiet hours span midnight.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct QuietHours {
    pub enabled: bool,
    pub from: u16,
    pub to: u16,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            from: 22 * 60,
            to: 7 * 60,
        }
    }
}

impl QuietHours {
    /// Is it quiet at this time of day (minutes since midnight)?
    pub fn contains(&self, minute: u16) -> bool {
        if !self.enabled || self.from == self.to {
            return false;
        }
        let minute = minute % DAY;
        if self.from < self.to {
            (self.from..self.to).contains(&minute)
        } else {
            minute >= self.from || minute < self.to
        }
    }

    /// Both times lie within one day.
    pub fn is_valid(&self) -> bool {
        self.from < DAY && self.to < DAY
    }

    /// `22:00`
    pub fn format(minute: u16) -> String {
        format!("{:02}:{:02}", (minute % DAY) / 60, minute % 60)
    }

    /// Reads `HH:MM`. Invalid input gives `None`.
    pub fn parse(text: &str) -> Option<u16> {
        let (h, m) = text.trim().split_once(':')?;
        let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some(h * 60 + m)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
struct Rule {
    reason: NotifyReason,
    #[serde(default)]
    channels: Channels,
}

/// Settings per reason and channel, plus the quiet hours.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct NotifySettings {
    /// Only deviating reasons; if one is missing, [`NotifySettings::default_channels`] applies.
    #[serde(default)]
    rules: Vec<Rule>,
    #[serde(default)]
    pub quiet: QuietHours,
}

impl NotifySettings {
    /// Default: everything on in the browser.
    pub fn default_channels() -> Channels {
        Channels { browser: true }
    }

    pub fn channels(&self, reason: NotifyReason) -> Channels {
        self.rules
            .iter()
            .find(|r| r.reason == reason)
            .map_or_else(Self::default_channels, |r| r.channels)
    }

    pub fn enabled(&self, reason: NotifyReason, channel: Channel) -> bool {
        self.channels(reason).get(channel)
    }

    /// Switches a channel for a reason. `false` if nothing changes.
    pub fn set(&mut self, reason: NotifyReason, channel: Channel, on: bool) -> bool {
        let mut channels = self.channels(reason);
        if channels.get(channel) == on {
            return false;
        }
        channels.set(channel, on);
        match self.rules.iter_mut().find(|r| r.reason == reason) {
            Some(rule) => rule.channels = channels,
            None => self.rules.push(Rule { reason, channels }),
        }
        true
    }

    /// Sets the quiet hours. `false` if they are invalid (from 24:00) or nothing changes.
    pub fn set_quiet(&mut self, quiet: QuietHours) -> bool {
        if !quiet.is_valid() || self.quiet == quiet {
            return false;
        }
        self.quiet = quiet;
        true
    }
}

/// An event one can be notified about.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub reason: NotifyReason,
    pub title: Msg,
    pub text: Msg,
    /// Target as an address of the app, e.g. `#/review/87`.
    pub route: String,
}

/// An event on a channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub channel: Channel,
    pub event: Event,
}

/// Which events go over which channels. `minute` is the local time in minutes since
/// midnight; during quiet hours nothing goes out.
pub fn deliveries(events: &[Event], settings: &NotifySettings, minute: u16) -> Vec<Delivery> {
    if settings.quiet.contains(minute) {
        return Vec::new();
    }
    events
        .iter()
        .flat_map(|event| {
            let channels = settings.channels(event.reason);
            Channel::ALL
                .into_iter()
                .filter(move |c| channels.get(*c))
                .map(|channel| Delivery {
                    channel,
                    event: event.clone(),
                })
        })
        .collect()
}

/// Is `@Name` or `@Initials` of the person in the text? Case does not matter.
pub fn mentions(text: &str, who: &Person) -> bool {
    let handles = who.handles();
    text.split('@').skip(1).any(|rest| {
        let word: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        !word.is_empty() && handles.contains(&word.to_lowercase())
    })
}

fn route_of(item: &Item) -> String {
    match &item.pr {
        Some(pr) => format!("#/review/{}", pr.key().route()),
        None => "#/".into(),
    }
}

/// The entry newly got this reason (it was not there before, different or done).
fn fresh(before: Option<&Item>, after: &Item, reason: ReasonKind) -> bool {
    after.reason == reason && !after.read && !before.is_some_and(|b| b.reason == reason && !b.read)
}

fn run_of<'a>(ws: &'a Workspace, key: &str) -> Option<&'a AgentRun> {
    ws.runs.iter().find(|r| r.key() == key)
}

/// The events that arose between two states of the workspace.
///
/// Without a "before" there is no comparison – the first state on loading therefore reports nothing.
pub fn events(before: &Workspace, after: &Workspace) -> Vec<Event> {
    let me = Person::me();
    let mut out = Vec::new();

    for item in &after.items {
        let old = before.item(&item.id);

        if fresh(old, item, ReasonKind::ReviewRequested) {
            out.push(Event {
                reason: NotifyReason::ReviewRequested,
                title: msg!(CoreNotifyReviewTitle, title = &item.title),
                text: msg!(
                    CoreNotifyReviewText,
                    key = &item.key,
                    reason = item.reason_line()
                ),
                route: route_of(item),
            });
        }
        if fresh(old, item, ReasonKind::Assigned) {
            out.push(Event {
                reason: NotifyReason::Assigned,
                title: msg!(CoreNotifyAssignedTitle, key = &item.key),
                text: Msg::raw(item.title.clone()),
                route: route_of(item),
            });
        }

        let seen = old.map_or(0, |o| o.comments.len());
        for comment in item.comments.iter().skip(seen) {
            if !comment.own && !comment.author.is_agent() && mentions(&comment.text, &me) {
                out.push(Event {
                    reason: NotifyReason::Mention,
                    title: msg!(CoreNotifyMentionTitle, name = &comment.author.name),
                    text: msg!(CoreNotifyMentionText, key = &item.key, text = &comment.text),
                    route: route_of(item),
                });
            }
        }

        let checks_failed = |i: &Item| i.pr.as_ref().is_some_and(|p| p.checks == Checks::Failed);
        if item.scope == Scope::Mine && checks_failed(item) && !old.is_some_and(checks_failed) {
            let pr = item.pr.as_ref().map(PrRef::key).unwrap_or_default();
            out.push(Event {
                reason: NotifyReason::ChecksFailed,
                title: msg!(CoreNotifyChecksTitle, pr = pr.to_string()),
                text: Msg::raw(item.title.clone()),
                route: format!("#/review/{}", pr.route()),
            });
        }
    }

    for run in &after.runs {
        let old = run_of(before, &run.key());
        let (key, label) = (run.key(), run.label_msg());
        if run.is_awaiting() && !old.is_some_and(AgentRun::is_awaiting) {
            out.push(Event {
                reason: NotifyReason::AgentWaiting,
                title: msg!(CoreNotifyWaitingTitle, label = &label),
                text: msg!(CoreNotifyPlanText),
                route: format!("#/agent/{key}"),
            });
        }
        if run.is_asking() && !old.is_some_and(AgentRun::is_asking) {
            out.push(Event {
                reason: NotifyReason::AgentWaiting,
                title: msg!(CoreNotifyAsksTitle, label = &label),
                text: run
                    .question
                    .as_ref()
                    .map_or_else(|| msg!(CoreNotifyAsksDefault), |q| Msg::raw(q.text.clone())),
                route: format!("#/agent/{key}"),
            });
        }
        if let Some(reason) = run.stop_reason()
            && old.and_then(AgentRun::stop_reason) != Some(reason)
        {
            out.push(Event {
                reason: NotifyReason::AgentStopped,
                title: msg!(CoreNotifyStoppedTitle, key = &run.ticket_key),
                text: msg!(CoreNotifyStoppedText, reason = reason.label_msg()),
                route: format!("#/agent/{}", run.ticket_key),
            });
        }
        if run.is_done() && !old.is_some_and(AgentRun::is_done) {
            let (text, route) = match run.pr_key() {
                Some(n) if run.is_followup() => (
                    msg!(
                        CoreNotifyDoneFollowup,
                        title = &run.ticket_title,
                        pr = n.to_string()
                    ),
                    format!("#/review/{}", n.route()),
                ),
                _ => (Msg::raw(run.ticket_title.clone()), format!("#/agent/{key}")),
            };
            out.push(Event {
                reason: NotifyReason::AgentFinished,
                title: msg!(CoreNotifyDoneTitle, label = &label),
                text,
                route,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests;
