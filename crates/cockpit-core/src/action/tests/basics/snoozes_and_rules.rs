use super::*;
use cockpit_i18n::Lang;

#[test]
fn snoozing_is_an_action_and_comes_back_by_itself() {
    let mut w = ws();
    let hour = 3_600_000;
    let applied = ok(
        &mut w,
        Action::Snooze {
            item: "pr87".into(),
            until_ms: Some(T + hour),
        },
        1,
    );
    assert_eq!(first_text(&w, &applied), "#87 zurückgestellt");
    assert!(w.is_snoozed("pr87", T));
    // Nothing to wake as long as the time is not up.
    fails(&mut w, Action::WakeSnoozed, ActionError::NothingToDo);
    // The tick brings it back and logs it as System.
    let woke = w
        .apply(Action::WakeSnoozed, Actor::System, T + hour)
        .unwrap();
    assert_eq!(first_text(&w, &woke), "#87 ist zurück in der Inbox");
    assert!(w.snoozed.is_empty());
    assert_eq!(w.log.last().unwrap().actor, Actor::System);

    ok(
        &mut w,
        Action::Snooze {
            item: "pr87".into(),
            until_ms: None,
        },
        1,
    );
    assert_eq!(
        w.log.last().unwrap().text_in(Lang::De),
        "#87 zurückgestellt, bis sich etwas ändert"
    );
    ok(
        &mut w,
        Action::Unsnooze {
            item: "pr87".into(),
        },
        1,
    );
    fails(
        &mut w,
        Action::Unsnooze {
            item: "pr87".into(),
        },
        ActionError::NothingToDo,
    );
    fails(
        &mut w,
        Action::Snooze {
            item: "gibt-es-nicht".into(),
            until_ms: None,
        },
        ActionError::UnknownItem("gibt-es-nicht".into()),
    );
    // A point in time in the past is not a snooze.
    fails(
        &mut w,
        Action::Snooze {
            item: "pr87".into(),
            until_ms: Some(T),
        },
        ActionError::InvalidInput,
    );
}

#[test]
fn a_change_wakes_a_snooze_and_the_tick_logs_it() {
    let mut w = ws();
    ok(
        &mut w,
        Action::Snooze {
            item: "bau142".into(),
            until_ms: None,
        },
        1,
    );
    fails(&mut w, Action::WakeSnoozed, ActionError::NothingToDo);
    ok(
        &mut w,
        Action::Reply {
            item: "bau142".into(),
            text: "Mache ich.".into(),
        },
        1,
    );
    let woke = w.apply(Action::WakeSnoozed, Actor::System, T).unwrap();
    assert_eq!(first_text(&w, &woke), "BAU-142 ist zurück in der Inbox");
}

#[test]
fn rules_are_switched_by_action() {
    let mut w = ws();
    let off = ok(
        &mut w,
        Action::SetRule {
            rule: Rule::Overdue,
            enabled: false,
        },
        1,
    );
    assert_eq!(
        first_text(&w, &off),
        "Regel „Überfälliges Ticket“ ausgeschaltet"
    );
    assert!(!w.rules.enabled(Rule::Overdue));
    assert_eq!(w.log.last().unwrap().target, None);
    fails(
        &mut w,
        Action::SetRule {
            rule: Rule::Overdue,
            enabled: false,
        },
        ActionError::Unchanged,
    );
    let on = ok(
        &mut w,
        Action::SetRule {
            rule: Rule::Overdue,
            enabled: true,
        },
        1,
    );
    assert_eq!(
        first_text(&w, &on),
        "Regel „Überfälliges Ticket“ eingeschaltet"
    );
}

#[test]
fn rules_and_snoozes_survive_save_and_load() {
    let mut w = ws();
    w.apply(
        Action::SetRule {
            rule: Rule::Assigned,
            enabled: false,
        },
        Actor::User,
        T,
    )
    .unwrap();
    w.apply(
        Action::Snooze {
            item: "pr87".into(),
            until_ms: Some(T + 1),
        },
        Actor::User,
        T,
    )
    .unwrap();
    let back = loaded(&saved(&w)).unwrap();
    assert_eq!(back, w);
    assert!(!back.rules.enabled(Rule::Assigned));
    let text = serde_json::to_string(&Action::Snooze {
        item: "x".into(),
        until_ms: None,
    })
    .unwrap();
    assert!(matches!(
        serde_json::from_str::<Action>(&text),
        Ok(Action::Snooze { until_ms: None, .. })
    ));
}
