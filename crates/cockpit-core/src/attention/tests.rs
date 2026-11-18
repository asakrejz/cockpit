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
    assert!(!mentions_me("Du hast Post"), "no mention without @");
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
        "time alone does not wake"
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
