use super::*;
use crate::model::{Comment, PrRef};
use crate::{AgentOptions, demo};

fn ws() -> Workspace {
    demo::workspace()
}

fn only(events: Vec<Event>) -> Event {
    assert_eq!(events.len(), 1, "{events:?}");
    events.into_iter().next().unwrap()
}

#[test]
fn unchanged_workspace_reports_nothing() {
    let w = ws();
    assert!(events(&w, &w).is_empty());
}

#[test]
fn new_review_request_is_reported_once() {
    let before = ws();
    let mut after = before.clone();
    let mut item = after.item("pr87").unwrap().clone();
    item.id = "pr99".into();
    item.key = "#99".into();
    item.title = "Neuer Button".into();
    item.pr = Some(PrRef {
        repo: String::new(),
        number: 99,
        draft: false,
        checks: Checks::Passed,
        changes_requested: false,
        merged: false,
        review_requested: false,
        author: String::new(),
        mine: false,
        requested_me: false,
        assigned_me: false,
    });
    after.items.push(item);
    let e = only(events(&before, &after));
    assert_eq!(e.reason, NotifyReason::ReviewRequested);
    assert_eq!(e.route, "#/review/99");
    assert!(e.title.contains("Neuer Button"));
    // Not again afterwards.
    assert!(events(&after, &after).is_empty());
}

#[test]
fn done_item_that_returns_counts_again() {
    let mut before = ws();
    before.mark_done("pr87");
    let mut after = before.clone();
    after
        .items
        .iter_mut()
        .find(|i| i.id == "pr87")
        .unwrap()
        .read = false;
    assert_eq!(
        only(events(&before, &after)).reason,
        NotifyReason::ReviewRequested
    );
}

#[test]
fn assignment_is_reported() {
    let before = ws();
    let mut after = before.clone();
    let item = after.items.iter_mut().find(|i| i.id == "bau142").unwrap();
    item.reason = ReasonKind::Assigned;
    let e = only(events(&before, &after));
    assert_eq!(e.reason, NotifyReason::Assigned);
    assert!(e.title.contains("BAU-142"));
    assert_eq!(e.route, "#/");
}

#[test]
fn mention_needs_a_new_foreign_comment_with_my_name() {
    let before = ws();
    let mut after = before.clone();
    let lena = Person::new("Lena", "LK");
    fn comments(w: &mut Workspace) -> &mut Vec<Comment> {
        &mut w
            .items
            .iter_mut()
            .find(|i| i.id == "bau142")
            .unwrap()
            .comments
    }
    comments(&mut after).push(Comment::new(lena.clone(), "gerade eben", "Ohne Namen."));
    assert!(events(&before, &after).is_empty());
    comments(&mut after).push(Comment::new(lena, "gerade eben", "Was meinst du, @du?"));
    let e = only(events(&before, &after));
    assert_eq!(e.reason, NotifyReason::Mention);
    assert!(e.title.contains("Lena"));
    // Own comments and old comments report nothing.
    let mut own = before.clone();
    own.reply("bau142", "@Du bin das selbst");
    assert!(events(&before, &own).is_empty());
}

#[test]
fn mention_matching() {
    let me = Person::me();
    assert!(mentions("Hi @Du!", &me));
    assert!(mentions("@as bitte prüfen", &me));
    assert!(mentions("a@b.de und @DU", &me));
    assert!(!mentions("Mail an a@b.de", &me));
    assert!(!mentions("@Dunkel", &me));
    assert!(!mentions("@", &me));
    assert!(!mentions("ohne", &me));
}

#[test]
fn checks_turning_red_on_my_pr() {
    let mut before = ws();
    let (id, number) = {
        let i = before
            .items
            .iter_mut()
            .find(|i| i.pr.is_some() && i.scope == Scope::Mine)
            .expect("an own pull request in the sample data");
        i.pr.as_mut().unwrap().checks = Checks::Running;
        (i.id.clone(), i.pr.as_ref().unwrap().number)
    };
    let mut after = before.clone();
    after
        .items
        .iter_mut()
        .find(|i| i.id == id)
        .unwrap()
        .pr
        .as_mut()
        .unwrap()
        .checks = Checks::Failed;
    let e = only(events(&before, &after));
    assert_eq!(e.reason, NotifyReason::ChecksFailed);
    assert_eq!(e.route, format!("#/review/{}/{number}", crate::DEMO_REPO));
    // Already been red: no second notification.
    assert!(events(&after, &after).is_empty());
}

#[test]
fn checks_of_team_prs_are_not_mine() {
    let before = ws();
    let mut after = before.clone();
    if let Some(i) = after
        .items
        .iter_mut()
        .find(|i| i.pr.is_some() && i.scope == Scope::Team)
    {
        i.pr.as_mut().unwrap().checks = Checks::Failed;
        assert!(
            events(&before, &after)
                .iter()
                .all(|e| e.reason != NotifyReason::ChecksFailed)
        );
    }
}

#[test]
fn agent_question_is_reported_once() {
    let mut before = ws();
    let direct = AgentOptions {
        plan_first: false,
        ..AgentOptions::default()
    };
    before.start_agent("BAU-142", direct).unwrap();
    let mut after = before.clone();
    let mut found = None;
    for _ in 0..crate::agent::STEPS {
        let prev = after.clone();
        after.advance_agents();
        let list = events(&prev, &after);
        if !list.is_empty() {
            found = Some(only(list));
            break;
        }
    }
    let e = found.expect("follow-up question is reported");
    assert_eq!(e.reason, NotifyReason::AgentWaiting);
    assert_eq!(e.route, "#/agent/BAU-142");
    assert!(e.text.contains("deprecated"), "{e:?}");
    // As long as it is open, it does not come again.
    assert!(events(&after, &after.clone()).is_empty());
}

#[test]
fn agent_waiting_and_finished() {
    let before = ws();
    let key = before
        .items
        .iter()
        // Without a follow-up question in the demo, so that the run goes through.
        .find(|i| i.is_ticket() && i.pr.is_none() && demo::agent_question(&i.key).is_none())
        .unwrap()
        .key
        .clone();

    let mut waiting = before.clone();
    waiting.start_agent(&key, AgentOptions::default()).unwrap();
    let e = only(events(&before, &waiting));
    assert_eq!(e.reason, NotifyReason::AgentWaiting);
    assert_eq!(e.route, format!("#/agent/{key}"));

    let mut running = waiting.clone();
    assert!(running.approve_plan(&key));
    assert!(
        events(&waiting, &running).is_empty(),
        "approval reports nothing"
    );

    let mut after = running.clone();
    for _ in 0..crate::agent::STEPS - 1 {
        after.advance_agents();
        assert!(events(&running, &after).is_empty(), "quiet on the way");
    }
    let prev = after.clone();
    after.advance_agents();
    let e = only(events(&prev, &after));
    assert_eq!(e.reason, NotifyReason::AgentFinished);
    assert!(e.route.starts_with("#/review/"), "{}", e.route);
    assert!(e.text.contains("Review"));
}

#[test]
fn agent_without_plan_only_reports_when_finished() {
    let before = ws();
    let key = before
        .items
        .iter()
        .find(|i| i.is_ticket() && i.pr.is_none())
        .unwrap()
        .key
        .clone();
    let mut after = before.clone();
    let direct = AgentOptions {
        plan_first: false,
        ..AgentOptions::default()
    };
    after.start_agent(&key, direct).unwrap();
    assert!(events(&before, &after).is_empty());
}

#[test]
fn defaults_are_browser_only() {
    let s = NotifySettings::default();
    for reason in NotifyReason::ALL {
        assert!(s.enabled(reason, Channel::Browser));
        assert!(!s.enabled(reason, Channel::Email));
        assert!(!s.enabled(reason, Channel::Slack));
    }
    assert!(!s.quiet.enabled);
}

#[test]
fn channels_are_set_per_reason() {
    let mut s = NotifySettings::default();
    assert!(s.set(NotifyReason::Mention, Channel::Email, true));
    assert!(
        !s.set(NotifyReason::Mention, Channel::Email, true),
        "nothing changes"
    );
    assert!(s.set(NotifyReason::Mention, Channel::Browser, false));
    assert!(s.enabled(NotifyReason::Mention, Channel::Email));
    assert!(!s.enabled(NotifyReason::Mention, Channel::Browser));
    assert!(
        s.enabled(NotifyReason::Assigned, Channel::Browser),
        "andere bleiben"
    );
}

#[test]
fn quiet_hours_over_midnight_and_within_a_day() {
    let night = QuietHours {
        enabled: true,
        from: 22 * 60,
        to: 7 * 60,
    };
    assert!(night.contains(23 * 60) && night.contains(0) && night.contains(6 * 60 + 59));
    assert!(!night.contains(7 * 60) && !night.contains(12 * 60) && !night.contains(21 * 60 + 59));
    assert!(night.contains(22 * 60));
    let lunch = QuietHours {
        enabled: true,
        from: 12 * 60,
        to: 13 * 60,
    };
    assert!(lunch.contains(12 * 60 + 30) && !lunch.contains(13 * 60) && !lunch.contains(11 * 60));
    assert!(
        !QuietHours {
            enabled: false,
            ..night
        }
        .contains(0)
    );
    assert!(
        !QuietHours {
            enabled: true,
            from: 60,
            to: 60
        }
        .contains(60),
        "same time: no quiet period"
    );
}

#[test]
fn quiet_times_format_and_parse() {
    assert_eq!(QuietHours::format(22 * 60 + 5), "22:05");
    assert_eq!(QuietHours::parse("07:30"), Some(450));
    assert_eq!(QuietHours::parse(" 7:05 "), Some(425));
    for bad in ["", "24:00", "12:60", "abc", "12", "-1:00", "12:xx"] {
        assert_eq!(QuietHours::parse(bad), None, "{bad}");
    }
    for m in [0, 59, 60, 1439] {
        assert_eq!(QuietHours::parse(&QuietHours::format(m)), Some(m));
    }
}

#[test]
fn deliveries_follow_settings_and_quiet_hours() {
    let event = |reason| Event {
        reason,
        title: "t".into(),
        text: "x".into(),
        route: "#/".into(),
    };
    let evs = [event(NotifyReason::Mention), event(NotifyReason::Assigned)];
    let mut s = NotifySettings::default();
    s.set(NotifyReason::Mention, Channel::Slack, true);
    s.set(NotifyReason::Assigned, Channel::Browser, false);
    let got: Vec<_> = deliveries(&evs, &s, 12 * 60)
        .into_iter()
        .map(|d| (d.event.reason, d.channel))
        .collect();
    assert_eq!(
        got,
        [
            (NotifyReason::Mention, Channel::Browser),
            (NotifyReason::Mention, Channel::Slack)
        ]
    );
    s.quiet = QuietHours {
        enabled: true,
        from: 22 * 60,
        to: 7 * 60,
    };
    assert!(deliveries(&evs, &s, 23 * 60).is_empty());
    assert_eq!(deliveries(&evs, &s, 8 * 60).len(), 2);
}

#[test]
fn ids_roundtrip() {
    for r in NotifyReason::ALL {
        assert_eq!(NotifyReason::from_id(r.id()), Some(r));
    }
    for c in Channel::ALL {
        assert_eq!(Channel::from_id(c.id()), Some(c));
    }
    assert_eq!(NotifyReason::from_id("?"), None);
    assert!(Channel::Email.via_backend() && !Channel::Browser.via_backend());
}

#[test]
fn settings_json_is_stable_and_old_data_loads() {
    let mut s = NotifySettings::default();
    s.set(NotifyReason::Mention, Channel::Email, true);
    let text = serde_json::to_string(&s).unwrap();
    assert_eq!(serde_json::from_str::<NotifySettings>(&text).unwrap(), s);
    assert_eq!(
        serde_json::from_str::<NotifySettings>("{}").unwrap(),
        NotifySettings::default()
    );
    assert!(
        !text.contains('@') && !text.contains("http"),
        "no addresses"
    );
}
