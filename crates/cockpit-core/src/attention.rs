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
mod tests {
    use super::*;
    use crate::{Comment, Person, demo};

    /// 11.11.2026, 12:00 UTC (a Wednesday).
    fn now() -> u64 {
        day_start_ms("2026-11-11").unwrap() + 12 * HOUR_MS
    }

    fn ticket() -> Item {
        demo::workspace().item("bau150").unwrap().clone()
    }

    fn quiet(mut item: Item) -> Item {
        item.due = None;
        item.owner = Person::new("Mira", "MT");
        item.scope = Scope::Team;
        item.pr = None;
        item.comments.clear();
        item.reason = ReasonKind::Comment;
        item
    }

    fn rules_of(item: &Item) -> Vec<Rule> {
        hits(item, &Rules::default(), now())
            .into_iter()
            .map(|h| h.rule)
            .collect()
    }

    #[test]
    fn days_are_parsed_strictly() {
        assert_eq!(parse_day("1970-01-01"), Some(0));
        assert_eq!(parse_day("1970-01-02"), Some(1));
        assert_eq!(parse_day("2000-03-01"), Some(11_017));
        assert_eq!(parse_day("2026-11-11"), Some(20_768));
        assert_eq!(parse_day("1969-12-31"), Some(-1));
        for bad in [
            "",
            "2026",
            "2026-13-01",
            "2026-02-29",
            "2026-1-1",
            "morgen",
            "2026-11-11-1",
            "2026-00-10",
        ] {
            assert_eq!(parse_day(bad), None, "{bad}");
        }
        assert!(parse_day("2028-02-29").is_some(), "Schaltjahr");
    }

    #[test]
    fn mention_needs_a_whole_name() {
        assert!(mentions_me("@AS schau mal"));
        assert!(mentions_me("Hallo @du, bitte prüfen"));
        assert!(mentions_me("danke @Du!"));
        assert!(mentions_me("@as"));
        assert!(!mentions_me("@Dutzend Fehler"));
        assert!(!mentions_me("Du hast Post"), "ohne @ keine Erwähnung");
    }

    #[test]
    fn a_mention_counts_until_i_answer() {
        let mut item = quiet(ticket());
        assert!(rules_of(&item).is_empty());
        item.comments.push(Comment::new(
            Person::new("Lena", "LK"),
            "gerade",
            "@AS kannst du?",
        ));
        let found = hits(&item, &Rules::default(), now());
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].rule, found[0].reason, found[0].text.as_str()),
            (Rule::Mention, ReasonKind::Mention, "Lena hat dich erwähnt")
        );
        // Own mentions and others' without me do not count.
        let mut own = Comment::new(Person::me(), "gerade", "@AS Notiz an mich");
        own.own = true;
        let mut item2 = quiet(ticket());
        item2.comments.push(own);
        item2.comments.push(Comment::new(
            Person::new("Lena", "LK"),
            "gerade",
            "@Jonas bitte",
        ));
        assert!(rules_of(&item2).is_empty());
        // After my answer it is done.
        let mut reply = Comment::new(Person::me(), "gerade", "Mache ich.");
        reply.own = true;
        item.comments.push(reply);
        assert!(rules_of(&item).is_empty());
        // A new mention afterwards counts again.
        item.comments.push(Comment::new(
            Person::new("Jonas", "JB"),
            "gerade",
            "Nochmal @du",
        ));
        assert_eq!(
            hits(&item, &Rules::default(), now())[0].text,
            "Jonas hat dich erwähnt"
        );
    }

    #[test]
    fn review_requests_come_from_others() {
        let w = demo::workspace();
        let pr = w.item("pr87").unwrap();
        let found = hits(pr, &Rules::default(), now());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, Rule::ReviewRequested);
        assert_eq!(found[0].reason, ReasonKind::ReviewRequested);
        assert_eq!(found[0].text, "Review angefragt von Jonas");
        // A ticket with this reason does not count.
        let mut ticket = quiet(ticket());
        ticket.reason = ReasonKind::ReviewRequested;
        assert!(rules_of(&ticket).is_empty());
    }

    #[test]
    fn my_pull_request_is_blocked_by_red_checks_or_requested_changes() {
        let w = demo::workspace();
        // Changes requested (BAU-138 with #91).
        let a = hits(w.item("bau138").unwrap(), &Rules::default(), now());
        assert_eq!(a[0].rule, Rule::Blocked);
        assert_eq!(
            (a[0].reason, a[0].text.as_str()),
            (
                ReasonKind::ChangesRequested,
                "Änderungen angefordert in #91"
            )
        );
        // Red checks on my pull request.
        let mut item = w.item("bau138").unwrap().clone();
        let pr = item.pr.as_mut().unwrap();
        pr.changes_requested = false;
        pr.checks = Checks::Failed;
        let b = check(Rule::Blocked, &item, now()).unwrap();
        assert_eq!(
            (b.reason, b.text.as_str()),
            (ReasonKind::ChecksFailed, "Checks fehlgeschlagen in #91")
        );
        // Green or running checks do not block.
        for ok in [Checks::Passed, Checks::Running] {
            item.pr.as_mut().unwrap().checks = ok;
            assert_eq!(check(Rule::Blocked, &item, now()), None);
        }
        // Mira's red pull request is not mine.
        assert_eq!(check(Rule::Blocked, w.item("pr85").unwrap(), now()), None);
    }

    #[test]
    fn overdue_means_a_due_day_before_today() {
        let mut item = quiet(ticket());
        item.due = Some("2026-11-09".into());
        assert_eq!(
            check(Rule::Overdue, &item, now()).unwrap().text,
            "Seit 2 Tagen überfällig"
        );
        item.due = Some("2026-11-10".into());
        let hit = check(Rule::Overdue, &item, now()).unwrap();
        assert_eq!(
            (hit.reason, hit.text.as_str()),
            (ReasonKind::Overdue, "Seit gestern überfällig")
        );
        // Due today is not yet overdue – until the last moment of the day.
        item.due = Some("2026-11-11".into());
        assert_eq!(check(Rule::Overdue, &item, now()), None);
        let end = day_start_ms("2026-11-12").unwrap() - 1;
        assert_eq!(check(Rule::Overdue, &item, end), None);
        assert!(check(Rule::Overdue, &item, end + 1).is_some());
        // Broken date, no date, done, no ticket.
        item.due = Some("gestern".into());
        assert_eq!(check(Rule::Overdue, &item, now()), None);
        item.due = None;
        assert_eq!(check(Rule::Overdue, &item, now()), None);
        item.due = Some("2026-11-01".into());
        item.status = TicketStatus::Done;
        assert_eq!(check(Rule::Overdue, &item, now()), None);
        let mut pr = demo::workspace().item("pr87").unwrap().clone();
        pr.due = Some("2026-11-01".into());
        assert_eq!(check(Rule::Overdue, &pr, now()), None);
    }

    #[test]
    fn assigned_tickets_are_mine_and_not_done() {
        let mut item = quiet(ticket());
        assert_eq!(check(Rule::Assigned, &item, now()), None);
        item.owner = Person::me();
        let hit = check(Rule::Assigned, &item, now()).unwrap();
        assert_eq!(
            (hit.reason, hit.text.as_str()),
            (ReasonKind::Assigned, "Dir zugewiesen")
        );
        item.status = TicketStatus::Done;
        assert_eq!(check(Rule::Assigned, &item, now()), None);
    }

    #[test]
    fn agent_results_wait_for_me() {
        let mut w = demo::workspace();
        let item = w.item("bau138").unwrap();
        let hit = check(Rule::AgentFinished, item, now()).unwrap();
        assert_eq!(hit.text, "Agent fertig · Pull Request #91 erstellt");
        w.set_status("bau138", TicketStatus::Done);
        assert_eq!(
            check(Rule::AgentFinished, w.item("bau138").unwrap(), now()),
            None
        );
    }

    #[test]
    fn every_rule_hits_somewhere_in_the_demo_data() {
        let w = demo::workspace();
        for rule in Rule::ALL {
            assert!(
                w.items.iter().any(|i| check(rule, i, now()).is_some()),
                "{rule:?} greift in den Demo-Daten nie"
            );
        }
    }

    #[test]
    fn hits_follow_the_rank_and_respect_the_switches() {
        let w = demo::workspace();
        let item = w.item("bau150").unwrap();
        let all: Vec<Rule> = rules_of(item);
        assert_eq!(all, [Rule::Overdue, Rule::Assigned]);
        let mut rules = Rules::default();
        assert!(rules.set(Rule::Overdue, false));
        assert!(!rules.set(Rule::Overdue, false), "schon aus");
        let rest = hits(item, &rules, now());
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].rule, Rule::Assigned);
        assert!(rules.set(Rule::Overdue, true));
        assert_eq!(rules, Rules::default());
    }

    #[test]
    fn rule_ids_roundtrip() {
        for rule in Rule::ALL {
            assert_eq!(Rule::from_id(rule.id()), Some(rule));
            assert!(!rule.label().is_empty() && !rule.describe().is_empty());
        }
        assert_eq!(Rule::from_id("?"), None);
        for (id, _) in SnoozeChoice::OPTIONS {
            assert_eq!(SnoozeChoice::from_id(id).map(SnoozeChoice::id), Some(*id));
        }
    }

    #[test]
    fn snooze_by_time_holds_until_then() {
        let item = ticket();
        let snooze = Snooze::new(&item, Some(now() + HOUR_MS));
        assert!(snooze.holds(&item, now()));
        assert!(snooze.holds(&item, now() + HOUR_MS - 1));
        assert!(!snooze.holds(&item, now() + HOUR_MS));
        // A change does not wake a timed snooze.
        let mut changed = item.clone();
        changed.status = TicketStatus::Done;
        assert!(snooze.holds(&changed, now()));
    }

    #[test]
    fn snooze_until_change_wakes_on_comment_status_or_checks() {
        let item = demo::workspace().item("bau138").unwrap().clone();
        let snooze = Snooze::new(&item, None);
        assert!(
            snooze.holds(&item, now() + 100 * DAY_MS),
            "Zeit allein weckt nicht"
        );

        let mut commented = item.clone();
        commented
            .comments
            .push(Comment::new(Person::new("Lena", "LK"), "jetzt", "Und?"));
        assert!(!snooze.holds(&commented, now()));

        let mut moved = item.clone();
        moved.status = TicketStatus::Done;
        assert!(!snooze.holds(&moved, now()));

        let mut checks = item.clone();
        checks.pr.as_mut().unwrap().checks = Checks::Failed;
        assert!(!snooze.holds(&checks, now()));
    }

    #[test]
    fn snooze_choices() {
        // Wednesday, 11.11.2026, 12:00 UTC; Berlin in winter is UTC+1, offset −60.
        let at = |s: &str, h: u64| day_start_ms(s).unwrap() + h * HOUR_MS;
        assert_eq!(
            SnoozeChoice::Hour.until_ms(now(), -60),
            Some(now() + HOUR_MS)
        );
        assert_eq!(
            SnoozeChoice::Tomorrow.until_ms(now(), -60),
            Some(at("2026-11-12", 7))
        );
        assert_eq!(
            SnoozeChoice::NextWeek.until_ms(now(), -60),
            Some(at("2026-11-16", 7))
        );
        assert_eq!(SnoozeChoice::UntilChange.until_ms(now(), -60), None);
        // Without time shift: 8 o'clock UTC.
        assert_eq!(
            SnoozeChoice::Tomorrow.until_ms(now(), 0),
            Some(at("2026-11-12", 8))
        );
        // Shortly after midnight local time (23:30 UTC = 00:30 in Berlin on the 12th): „tomorrow“ is the 13th.
        let late = at("2026-11-11", 23) + 30 * MINUTE_MS;
        assert_eq!(
            SnoozeChoice::Tomorrow.until_ms(late, -60),
            Some(at("2026-11-13", 7))
        );
        // On Monday „next week“ is the Monday after, never today.
        let monday = at("2026-11-16", 6);
        assert_eq!(
            SnoozeChoice::NextWeek.until_ms(monday, 0),
            Some(at("2026-11-23", 8))
        );
        // On Sunday already the day after.
        let sunday = at("2026-11-15", 20);
        assert_eq!(
            SnoozeChoice::NextWeek.until_ms(sunday, 0),
            Some(at("2026-11-16", 8))
        );
        // Always in the future.
        for choice in SnoozeChoice::ALL {
            for offset in [-780, -60, 0, 300, 720] {
                if let Some(t) = choice.until_ms(now(), offset) {
                    assert!(t > now(), "{choice:?} {offset}");
                }
            }
        }
    }
}
