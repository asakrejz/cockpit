//! Attention: *why* does an entry need me – and when not?
//!
//! Everything here is pure functions. The time comes from outside (`now_ms`, milliseconds
//! since 1970, UTC); `cockpit-core` knows no clock.
//!
//! * [`Rule`]: the rules, individually switchable ([`Rules`])
//! * [`hits`]: which rules apply to an entry, each with reason and a
//!   short German sentence; the first hit (by ranking) is the main reason
//! * [`Snooze`]: snooze an entry – for a time or „until something changes“
//! * [`SnoozeChoice`]: the four choices of the UI
//!
//! Due dates (`Item::due`) are days without time of day. „Today“ is the UTC day of
//! `now_ms`; around midnight this can deviate by the time zone.

use serde::{Deserialize, Serialize};

use crate::model::{Checks, Item, Kind, ReasonKind, Scope, TicketStatus};

const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 60 * MINUTE_MS;
const DAY_MS: u64 = 24 * HOUR_MS;

// ---------- Rules ----------

/// A rule by which an entry needs attention. The order is the
/// ranking: with several hits the first counts as the main reason.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    /// Someone mentioned me in a comment with `@Name`, and I did not answer.
    Mention,
    /// Someone asks me for a review.
    ReviewRequested,
    /// My pull request is blocked: checks red or changes requested.
    Blocked,
    /// The due date of a ticket lies in the past.
    Overdue,
    /// The agent has implemented a ticket; the pull request waits for me.
    AgentFinished,
    /// A ticket is assigned to me.
    Assigned,
}

impl Rule {
    pub const ALL: [Rule; 6] = [
        Rule::Mention,
        Rule::ReviewRequested,
        Rule::Blocked,
        Rule::Overdue,
        Rule::AgentFinished,
        Rule::Assigned,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Rule::Mention => "erwaehnung",
            Rule::ReviewRequested => "review",
            Rule::Blocked => "blockiert",
            Rule::Overdue => "ueberfaellig",
            Rule::AgentFinished => "agent",
            Rule::Assigned => "zugewiesen",
        }
    }

    pub fn from_id(id: &str) -> Option<Rule> {
        Rule::ALL.into_iter().find(|r| r.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Rule::Mention => "Erwähnung",
            Rule::ReviewRequested => "Review-Anfrage",
            Rule::Blocked => "Blockierter Pull Request",
            Rule::Overdue => "Überfälliges Ticket",
            Rule::AgentFinished => "Agent fertig",
            Rule::Assigned => "Zugewiesen",
        }
    }

    /// Explanation in one sentence, for the settings.
    pub fn describe(self) -> &'static str {
        match self {
            Rule::Mention => "Jemand erwähnt dich mit @Name und du hast noch nicht geantwortet",
            Rule::ReviewRequested => "Dein Review wird für einen Pull Request angefragt",
            Rule::Blocked => "Dein Pull Request hat rote Checks oder angeforderte Änderungen",
            Rule::Overdue => "Das Fälligkeitsdatum eines Tickets ist vorbei",
            Rule::AgentFinished => "Der Agent hat ein Ticket umgesetzt",
            Rule::Assigned => "Ein offenes Ticket ist dir zugewiesen",
        }
    }
}

/// Which rules are switched on. By default everything is on.
///
/// Only the switched-off ones are stored: that way a rule added later
/// counts as switched on immediately in old data.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Rules {
    #[serde(default)]
    pub off: Vec<Rule>,
}

impl Rules {
    pub fn enabled(&self, rule: Rule) -> bool {
        !self.off.contains(&rule)
    }

    /// Switches a rule on or off. Returns `false` if nothing changes.
    pub fn set(&mut self, rule: Rule, enabled: bool) -> bool {
        if self.enabled(rule) == enabled {
            return false;
        }
        if enabled {
            self.off.retain(|r| *r != rule);
        } else {
            self.off.push(rule);
        }
        true
    }
}

/// A hit: which rule, which reason, what is in the inbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub rule: Rule,
    pub reason: ReasonKind,
    /// Short German sentence, e.g. „Lena hat dich erwähnt“.
    pub text: String,
}

/// All hits of the switched-on rules, in the ranking of the rules.
pub fn hits(item: &Item, rules: &Rules, now_ms: u64) -> Vec<Hit> {
    Rule::ALL
        .into_iter()
        .filter(|r| rules.enabled(*r))
        .filter_map(|r| check(r, item, now_ms))
        .collect()
}

/// Applies a single rule, regardless of whether it is switched on.
pub fn check(rule: Rule, item: &Item, now_ms: u64) -> Option<Hit> {
    let hit = |reason, text: String| Hit { rule, reason, text };
    let open_ticket = item.kind == Kind::Ticket && item.status != TicketStatus::Done;
    match rule {
        Rule::Mention => {
            let by = mentioned_by(item)?;
            Some(hit(ReasonKind::Mention, format!("{} hat dich erwähnt", by)))
        }
        Rule::ReviewRequested => (item.kind == Kind::PullRequest
            && item.reason == ReasonKind::ReviewRequested)
            .then(|| {
                hit(
                    ReasonKind::ReviewRequested,
                    format!("Review angefragt von {}", item.owner.name),
                )
            }),
        Rule::Blocked => {
            let pr = item.pr.as_ref().filter(|_| item.scope == Scope::Mine)?;
            if pr.changes_requested {
                Some(hit(
                    ReasonKind::ChangesRequested,
                    format!("Änderungen angefordert in #{}", pr.number),
                ))
            } else if pr.checks == Checks::Failed {
                Some(hit(
                    ReasonKind::ChecksFailed,
                    format!("Checks fehlgeschlagen in #{}", pr.number),
                ))
            } else {
                None
            }
        }
        Rule::Overdue => {
            let days = overdue_days(item, now_ms).filter(|_| open_ticket)?;
            Some(hit(
                ReasonKind::Overdue,
                match days {
                    1 => "Seit gestern überfällig".to_string(),
                    n => format!("Seit {n} Tagen überfällig"),
                },
            ))
        }
        Rule::AgentFinished => {
            let pr = item.pr.as_ref()?;
            (open_ticket && item.reason == ReasonKind::AgentFinished).then(|| {
                hit(
                    ReasonKind::AgentFinished,
                    format!("Agent fertig · Pull Request #{} erstellt", pr.number),
                )
            })
        }
        Rule::Assigned => (open_ticket && item.owner == crate::model::Person::me())
            .then(|| hit(ReasonKind::Assigned, "Dir zugewiesen".to_string())),
    }
}

/// Who wrote the last mention of me that I have not answered yet?
fn mentioned_by(item: &Item) -> Option<&str> {
    let after_own = item
        .comments
        .iter()
        .rposition(|c| c.own)
        .map_or(0, |n| n + 1);
    item.comments[after_own..]
        .iter()
        .rev()
        .find(|c| !c.own && mentions_me(&c.text))
        .map(|c| c.author.name.as_str())
}

/// Is `@Name` (or `@Kürzel`) of me in the text? Case does not matter; after the
/// name no letter or digit may follow (`@Dutzend` does not count).
pub fn mentions_me(text: &str) -> bool {
    let me = crate::model::Person::me();
    let lower = text.to_lowercase();
    [me.name, me.initials].iter().any(|handle| {
        let needle = format!("@{}", handle.to_lowercase());
        lower.match_indices(&needle).any(|(at, _)| {
            lower[at + needle.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        })
    })
}

/// How many days is the ticket overdue? `None` if nothing is due or the
/// date is not in the past.
fn overdue_days(item: &Item, now_ms: u64) -> Option<u64> {
    let due = parse_day(item.due.as_deref()?)?;
    let today = (now_ms / DAY_MS) as i64;
    (today > due).then(|| (today - due) as u64)
}

// ---------- Days ----------

/// `2026-11-09` → days since 1970-01-01. `None` for an invalid date.
pub fn parse_day(text: &str) -> Option<i64> {
    let mut parts = text.trim().split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let last = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=last).contains(&d) {
        return None;
    }
    // Days since 1970 after H. Hinnant („days_from_civil“).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Milliseconds (UTC) for the start of a day. For tests and demos.
pub fn day_start_ms(text: &str) -> Option<u64> {
    parse_day(text)
        .and_then(|d| u64::try_from(d).ok())
        .map(|d| d * DAY_MS)
}

// ---------- Snoozing ----------

/// A snoozed entry.
///
/// With `until_ms` it comes back at that time. Without (`None`) it waits until
/// something changes: a new comment, a different status or new check results. For that
/// the snooze remembers the state of back then.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Snooze {
    /// Internal ID of the entry.
    pub item: String,
    #[serde(default)]
    pub until_ms: Option<u64>,
    /// State at snoozing: number of comments, status, checks.
    #[serde(default)]
    pub comments: usize,
    #[serde(default)]
    pub status: TicketStatus,
    #[serde(default)]
    pub checks: Option<Checks>,
}

impl Snooze {
    pub fn new(item: &Item, until_ms: Option<u64>) -> Self {
        Self {
            item: item.id.clone(),
            until_ms,
            comments: item.comments.len(),
            status: item.status,
            checks: item.pr.as_ref().map(|p| p.checks),
        }
    }

    /// Has anything changed since snoozing?
    pub fn changed(&self, item: &Item) -> bool {
        item.comments.len() != self.comments
            || item.status != self.status
            || item.pr.as_ref().map(|p| p.checks) != self.checks
    }

    /// Does the snooze still hold the entry back now?
    pub fn holds(&self, item: &Item, now_ms: u64) -> bool {
        match self.until_ms {
            Some(until) => now_ms < until,
            None => !self.changed(item),
        }
    }
}

/// The four choices in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnoozeChoice {
    Hour,
    Tomorrow,
    NextWeek,
    UntilChange,
}

impl SnoozeChoice {
    /// ID and label, in display order.
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("stunde", "1 Stunde"),
        ("morgen", "Morgen früh"),
        ("woche", "Nächste Woche"),
        ("aenderung", "Bis sich etwas ändert"),
    ];

    pub const ALL: [SnoozeChoice; 4] = [
        SnoozeChoice::Hour,
        SnoozeChoice::Tomorrow,
        SnoozeChoice::NextWeek,
        SnoozeChoice::UntilChange,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SnoozeChoice::Hour => "stunde",
            SnoozeChoice::Tomorrow => "morgen",
            SnoozeChoice::NextWeek => "woche",
            SnoozeChoice::UntilChange => "aenderung",
        }
    }

    pub fn from_id(id: &str) -> Option<SnoozeChoice> {
        SnoozeChoice::ALL.into_iter().find(|c| c.id() == id)
    }

    /// From when does the entry come back? `None`: only on a change.
    ///
    /// „Morgen früh“ and „Nächste Woche“ (the next Monday) mean 8 o'clock local time.
    /// `utc_offset_min` is the difference UTC − local time in minutes, as
    /// `Date.getTimezoneOffset()` returns it in the browser (Berlin in winter: −60).
    pub fn until_ms(self, now_ms: u64, utc_offset_min: i32) -> Option<u64> {
        let offset = i64::from(utc_offset_min) * MINUTE_MS as i64;
        let local = now_ms as i64 - offset;
        let today = local.div_euclid(DAY_MS as i64);
        let morning = |days_ahead: i64| {
            let at = (today + days_ahead) * DAY_MS as i64 + 8 * HOUR_MS as i64 + offset;
            at.max(0) as u64
        };
        match self {
            SnoozeChoice::Hour => Some(now_ms + HOUR_MS),
            SnoozeChoice::Tomorrow => Some(morning(1)),
            SnoozeChoice::NextWeek => {
                // 1970-01-01 was a Thursday; 0 = Monday.
                let weekday = (today + 3).rem_euclid(7);
                Some(morning(7 - weekday))
            }
            SnoozeChoice::UntilChange => None,
        }
    }
}

#[cfg(test)]
mod tests;
