use super::*;

#[test]
fn every_action_variant_is_applied_and_logged() {
    let mut w = ws();
    let file = w.review(87).unwrap().files[0].name.clone();
    let id = "bau142".to_string();
    let key = "BAU-142".to_string();

    ok(&mut w, Action::MarkDone { item: id.clone() }, 1);
    ok(
        &mut w,
        Action::SetStatus {
            item: "bau150".into(),
            status: TicketStatus::Done,
        },
        1,
    );
    ok(
        &mut w,
        Action::CreateBranch {
            item: "bau150".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::Reply {
            item: id.clone(),
            text: "Mache ich.".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::Decide {
            review: 87.into(),
            suggestion: "a".into(),
            decision: Some(Decision::Approved),
        },
        1,
    );
    ok(
        &mut w,
        Action::EditSuggestion {
            review: 87.into(),
            suggestion: "a".into(),
            text: "Anders".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::AddLineComment {
            review: 87.into(),
            file,
            line: 0,
            text: "Warum?".into(),
            suggestion: None,
        },
        1,
    );
    let comment = w.review(87).unwrap().line_comments[0].id.clone();
    ok(
        &mut w,
        Action::EditLineComment {
            review: 87.into(),
            comment: comment.clone(),
            text: "Wieso?".into(),
            suggestion: None,
        },
        1,
    );
    ok(
        &mut w,
        Action::DeleteLineComment {
            review: 87.into(),
            comment,
        },
        1,
    );
    ok(
        &mut w,
        Action::SetVerdict {
            review: 87.into(),
            verdict: Verdict::Approve,
        },
        1,
    );
    ok(&mut w, Action::RequestAgentReview { review: 85.into() }, 2);
    for s in ["b", "c"] {
        w.decide(87, s, Some(Decision::Declined));
    }
    ok(
        &mut w,
        Action::SendReview {
            review: 87.into(),
            comment: String::new(),
        },
        1,
    );
    ok(
        &mut w,
        Action::StartAgent {
            ticket: key.clone(),
            options: AgentOptions::default(),
        },
        2,
    );
    ok(
        &mut w,
        Action::RevisePlan {
            ticket: key.clone(),
            note: "Doku".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::ApprovePlan {
            ticket: key.clone(),
        },
        1,
    );
    ok(&mut w, Action::AdvanceAgents, 1);
    ok(
        &mut w,
        Action::SteerAgent {
            run: key.clone(),
            text: "Auch den Slider".into(),
        },
        1,
    );
    ok(&mut w, Action::PauseAgent { run: key.clone() }, 1);
    ok(&mut w, Action::ResumeAgent { run: key.clone() }, 1);
    ok(&mut w, Action::AdvanceAgents, 2);
    ok(&mut w, Action::AdvanceAgents, 1);
    ok(
        &mut w,
        Action::AnswerAgent {
            run: key.clone(),
            text: "Entfernen".into(),
        },
        1,
    );
    ok(
        &mut w,
        Action::CancelAgent {
            ticket: key.clone(),
        },
        1,
    );
    ok(
        &mut w,
        Action::StartAgent {
            ticket: key.clone(),
            options: AgentOptions::default(),
        },
        2,
    );
    ok(&mut w, Action::RejectPlan { ticket: key }, 1);
    let reset = w.apply(Action::ResetDemo, Actor::User, T).unwrap();
    assert_eq!(reset.logged, 1);
    assert_eq!(
        w.log.len(),
        1,
        "nach dem Zurücksetzen beginnt das Protokoll neu"
    );
    assert_eq!(w.log[0].actor, Actor::System);
    assert_eq!(w.log[0].text, "Demo-Daten zurückgesetzt");
}

#[test]
fn entries_carry_time_actor_target_and_a_german_sentence() {
    let mut w = ws();
    w.apply(
        Action::MarkDone {
            item: "bau142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    let e = &w.log[0];
    assert_eq!(
        (e.id, e.at_ms, e.actor, e.target.clone(), e.text.as_str()),
        (
            1,
            T,
            Actor::User,
            Some(Target::Item("bau142".into())),
            "BAU-142 als erledigt markiert"
        )
    );
    w.apply(
        Action::SetStatus {
            item: "bau150".into(),
            status: TicketStatus::InProgress,
        },
        Actor::User,
        T + 5,
    )
    .unwrap();
    assert_eq!(w.log[1].id, 2);
    assert_eq!(
        w.log[1].text,
        "Status von BAU-150 auf „In Arbeit“ ggesetzt".replace("gg", "g")
    );
    assert_eq!(w.log[1].at_ms, T + 5);
}

#[test]
fn failures_are_returned_and_leave_no_entry() {
    let mut w = ws();
    fails(
        &mut w,
        Action::MarkDone {
            item: "gibt-es-nicht".into(),
        },
        ActionError::UnknownItem("gibt-es-nicht".into()),
    );
    w.mark_done("bau142");
    fails(
        &mut w,
        Action::MarkDone {
            item: "bau142".into(),
        },
        ActionError::Unchanged,
    );
    fails(
        &mut w,
        Action::SetStatus {
            item: "pr87".into(),
            status: TicketStatus::Done,
        },
        ActionError::NotATicket,
    );
    fails(
        &mut w,
        Action::SetStatus {
            item: "bau150".into(),
            status: ws().item("bau150").unwrap().status,
        },
        ActionError::Unchanged,
    );
    fails(
        &mut w,
        Action::CreateBranch {
            item: "pr87".into(),
        },
        ActionError::BranchExists,
    );
    fails(
        &mut w,
        Action::Reply {
            item: "bau142".into(),
            text: "  ".into(),
        },
        ActionError::InvalidInput,
    );
    fails(
        &mut w,
        Action::Decide {
            review: 1.into(),
            suggestion: "a".into(),
            decision: None,
        },
        ActionError::UnknownReview(1.into()),
    );
    fails(
        &mut w,
        Action::Decide {
            review: 87.into(),
            suggestion: "zzz".into(),
            decision: None,
        },
        ActionError::UnknownSuggestion("zzz".into()),
    );
    fails(
        &mut w,
        Action::AddLineComment {
            review: 87.into(),
            file: "nope.css".into(),
            line: 0,
            text: "x".into(),
            suggestion: None,
        },
        ActionError::InvalidInput,
    );
    fails(
        &mut w,
        Action::DeleteLineComment {
            review: 87.into(),
            comment: "c9".into(),
        },
        ActionError::UnknownComment("c9".into()),
    );
    fails(
        &mut w,
        Action::SendReview {
            review: 87.into(),
            comment: String::new(),
        },
        ActionError::Send(SendError::Undecided(3)),
    );
    fails(
        &mut w,
        Action::SendReview {
            review: 1.into(),
            comment: String::new(),
        },
        ActionError::UnknownReview(1.into()),
    );
    fails(
        &mut w,
        Action::StartAgent {
            ticket: "#87".into(),
            options: direct(),
        },
        ActionError::Agent(AgentError::UnknownTicket),
    );
    fails(
        &mut w,
        Action::ApprovePlan {
            ticket: "BAU-142".into(),
        },
        ActionError::NothingToDo,
    );
    fails(
        &mut w,
        Action::CancelAgent {
            ticket: "BAU-138".into(),
        },
        ActionError::NothingToDo,
    );
    fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
    assert!(w.log.is_empty());
    assert!(!ActionError::NothingToDo.message().is_empty());
}

#[test]
fn starting_twice_fails() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    fails(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        ActionError::Agent(AgentError::AlreadyRunning),
    );
}

#[test]
fn agent_steps_from_the_tick_are_logged_as_agent() {
    let mut w = ws();
    w.apply(
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    for i in 0..=STEPS {
        if w.run("BAU-142").unwrap().is_asking() {
            w.apply(
                Action::AnswerAgent {
                    run: "BAU-142".into(),
                    text: "Entfernen".into(),
                },
                Actor::User,
                T + 1 + i as u64,
            )
            .unwrap();
        }
        w.apply(Action::AdvanceAgents, Actor::Agent, T + 1 + i as u64)
            .unwrap();
    }
    let steps: Vec<_> = w
        .log_entries(Some(Actor::Agent))
        .into_iter()
        .rev()
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(
        steps,
        [
            "BAU-142: Ticket gelesen",
            "BAU-142: Branch erstellt",
            "BAU-142: Der Agent hat eine Frage und wartet auf deine Antwort",
            "BAU-142: Änderungen umgesetzt",
            "BAU-142: Tests ausgeführt",
            "BAU-142: Pull Request asakrejz/rust-playground#92 als Entwurf erstellt",
        ]
    );
    let last = w.log.last().unwrap();
    assert_eq!(
        last.target,
        Some(Target::Review(PrKey::new(crate::DEMO_REPO, 92)))
    );
    assert_eq!(w.log[1].target, Some(Target::Run("BAU-142".into())));
    assert_eq!(
        w.log_entries(Some(Actor::User)).len(),
        2,
        "start and answer"
    );
    assert_eq!(w.log_entries(None).len(), 8);
    assert_eq!(w.log_entries(Some(Actor::System)).len(), 0);
    assert_eq!(w.log_entries(None)[0].id, 8, "neueste zuerst");
    // After that nothing runs any more.
    assert_eq!(
        w.apply(Action::AdvanceAgents, Actor::Agent, T),
        Err(ActionError::NothingToDo)
    );
}

#[test]
fn the_two_rules_still_hold() {
    let mut w = ws();
    // Approval before sending.
    assert_eq!(
        w.apply(
            Action::SendReview {
                review: 87.into(),
                comment: String::new()
            },
            Actor::User,
            T
        ),
        Err(ActionError::Send(SendError::Undecided(
            w.review(87).unwrap().undecided()
        )))
    );
    // The agent never merges: no merge in the actions, the pull request waits as a review.
    w.apply(
        Action::StartAgent {
            ticket: "BAU-150".into(),
            options: direct(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    for _ in 0..STEPS {
        let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T);
    }
    assert!(!w.review(92).unwrap().sent);
}

#[test]
fn the_log_is_capped_and_ids_keep_rising() {
    let mut w = ws();
    for i in 0..(MAX_LOG + 20) {
        let status = if i % 2 == 0 {
            TicketStatus::Done
        } else {
            TicketStatus::Open
        };
        let _ = w.apply(
            Action::SetStatus {
                item: "bau150".into(),
                status,
            },
            Actor::User,
            T + i as u64,
        );
    }
    assert_eq!(w.log.len(), MAX_LOG);
    assert_eq!(w.log.last().unwrap().id, (MAX_LOG + 20) as u64);
    assert_eq!(w.log[0].id, 21);
}

#[test]
fn notification_settings_are_changed_by_actions() {
    let mut w = ws();
    let mention = |on| Action::SetNotifyChannel {
        reason: NotifyReason::Mention,
        channel: Channel::Slack,
        on,
    };
    let applied = ok(&mut w, mention(true), 1);
    assert_eq!(
        applied.summary,
        "Benachrichtigung „Erwähnung“ über Slack eingeschaltet"
    );
    assert!(
        w.notifications
            .enabled(NotifyReason::Mention, Channel::Slack)
    );
    assert_eq!(w.log.last().unwrap().target, Some(Target::Settings));
    fails(&mut w, mention(true), ActionError::Unchanged);
    let applied = ok(&mut w, mention(false), 1);
    assert!(applied.summary.ends_with("ausgeschaltet"));

    let quiet = QuietHours {
        enabled: true,
        from: 22 * 60,
        to: 7 * 60,
    };
    let applied = ok(&mut w, Action::SetQuietHours { quiet }, 1);
    assert_eq!(
        applied.summary,
        "Ruhezeiten von 22:00 bis 07:00 Uhr eingeschaltet"
    );
    fails(
        &mut w,
        Action::SetQuietHours { quiet },
        ActionError::Unchanged,
    );
    fails(
        &mut w,
        Action::SetQuietHours {
            quiet: QuietHours {
                to: 24 * 60,
                ..quiet
            },
        },
        ActionError::InvalidInput,
    );
    let off = QuietHours {
        enabled: false,
        ..quiet
    };
    let applied = ok(&mut w, Action::SetQuietHours { quiet: off }, 1);
    assert_eq!(applied.summary, "Ruhezeiten ausgeschaltet");
}

#[test]
fn reset_keeps_notification_settings() {
    let mut w = ws();
    ok(
        &mut w,
        Action::SetNotifyChannel {
            reason: NotifyReason::Assigned,
            channel: Channel::Browser,
            on: false,
        },
        1,
    );
    w.apply(Action::ResetDemo, Actor::User, T).unwrap();
    assert!(
        !w.notifications
            .enabled(NotifyReason::Assigned, Channel::Browser)
    );
}

#[test]
fn notification_settings_survive_save_and_load() {
    let mut w = ws();
    w.notifications
        .set(NotifyReason::AgentFinished, Channel::Email, true);
    let back = persist::load(&persist::save(&w)).unwrap();
    assert!(
        back.notifications
            .enabled(NotifyReason::AgentFinished, Channel::Email)
    );
    // Older states without the field still load, with the default values.
    let mut json: serde_json::Value = serde_json::from_str(&persist::save(&w)).unwrap();
    json["workspace"]
        .as_object_mut()
        .unwrap()
        .remove("notifications");
    let old = persist::load(&json.to_string()).unwrap();
    assert_eq!(old.notifications, NotifySettings::default());
}

#[test]
fn actions_roundtrip_as_json() {
    let action = Action::Decide {
        review: 87.into(),
        suggestion: "a".into(),
        decision: Some(Decision::Declined),
    };
    let text = serde_json::to_string(&action).unwrap();
    assert_eq!(serde_json::from_str::<Action>(&text).unwrap(), action);
    let start = Action::StartAgent {
        ticket: "BAU-1".into(),
        options: AgentOptions::default(),
    };
    let text = serde_json::to_string(&start).unwrap();
    assert_eq!(serde_json::from_str::<Action>(&text).unwrap(), start);
    assert_eq!(
        serde_json::from_str::<Action>("\"AdvanceAgents\"").unwrap(),
        Action::AdvanceAgents
    );
}

#[test]
fn the_log_survives_save_and_load() {
    let mut w = ws();
    w.apply(
        Action::MarkDone {
            item: "bau142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    w.apply(
        Action::RequestAgentReview { review: 85.into() },
        Actor::User,
        T + 1,
    )
    .unwrap();
    let back = persist::load(&persist::save(&w)).unwrap();
    assert_eq!(back.log, w.log);
    assert_eq!(back, w);
}

#[test]
fn old_saves_without_a_log_still_load() {
    let mut value: serde_json::Value = serde_json::from_str(&persist::save(&ws())).unwrap();
    value["workspace"].as_object_mut().unwrap().remove("log");
    let back = persist::load(&value.to_string()).unwrap();
    assert!(back.log.is_empty());
    assert_eq!(back.items.len(), 7);
    // And it continues.
    let mut back = back;
    back.apply(
        Action::MarkDone {
            item: "bau142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(back.log.len(), 1);
}

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
    assert_eq!(applied.summary, "#87 zurückgestellt");
    assert!(w.is_snoozed("pr87", T));
    // Nothing to wake as long as the time is not up.
    fails(&mut w, Action::WakeSnoozed, ActionError::NothingToDo);
    // The tick brings it back and logs it as System.
    let woke = w
        .apply(Action::WakeSnoozed, Actor::System, T + hour)
        .unwrap();
    assert_eq!(woke.summary, "#87 ist zurück in der Inbox");
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
        w.log.last().unwrap().text,
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
    assert_eq!(woke.summary, "BAU-142 ist zurück in der Inbox");
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
    assert_eq!(off.summary, "Regel „Überfälliges Ticket“ ausgeschaltet");
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
    assert_eq!(on.summary, "Regel „Überfälliges Ticket“ eingeschaltet");
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
    let back = persist::load(&persist::save(&w)).unwrap();
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

#[test]
fn old_saves_without_rules_snoozes_or_due_dates_still_load() {
    let mut value: serde_json::Value = serde_json::from_str(&persist::save(&ws())).unwrap();
    let workspace = value["workspace"].as_object_mut().unwrap();
    workspace.remove("rules");
    workspace.remove("snoozed");
    for item in workspace["items"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("due");
        if let Some(pr) = item.get_mut("pr").and_then(|p| p.as_object_mut()) {
            pr.remove("changes_requested");
        }
    }
    let back = persist::load(&value.to_string()).unwrap();
    assert!(Rule::ALL.iter().all(|r| back.rules.enabled(*r)));
    assert!(back.snoozed.is_empty());
    assert_eq!(back.item("bau150").unwrap().due, None);
}

#[test]
fn actor_ids_roundtrip() {
    for a in Actor::ALL {
        assert_eq!(Actor::from_id(a.id()), Some(a));
    }
    assert_eq!(Actor::from_id("x"), None);
}

#[test]
fn answering_continues_the_agent_and_is_logged() {
    let mut w = asking_ws();
    assert_eq!(
        w.item("bau142").unwrap().agent_note.as_deref(),
        Some("Der Agent hat eine Frage an dich")
    );
    let applied = ok(
        &mut w,
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: " Als deprecated markieren ".into(),
        },
        1,
    );
    assert_eq!(
        applied.summary,
        "Frage des Agenten für BAU-142 beantwortet: „Als deprecated markieren“"
    );
    let last = w.log.last().unwrap();
    assert_eq!(
        (last.actor, &last.target),
        (Actor::User, &Some(Target::Run("BAU-142".into())))
    );
    assert!(w.run("BAU-142").unwrap().is_running());
    assert_eq!(
        w.item("bau142").unwrap().agent_note.as_deref(),
        Some("Der Agent arbeitet an der Umsetzung")
    );
}

#[test]
fn invalid_answers_change_nothing() {
    let mut w = asking_ws();
    fails(
        &mut w,
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "  ".into(),
        },
        ActionError::InvalidInput,
    );
    let mut idle = ws();
    for action in [
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "Ja".into(),
        },
        Action::SteerAgent {
            run: "BAU-142".into(),
            text: "Ja".into(),
        },
        Action::PauseAgent {
            run: "BAU-142".into(),
        },
        Action::ResumeAgent {
            run: "BAU-142".into(),
        },
    ] {
        fails(&mut idle, action, ActionError::NothingToDo);
    }
    // Whoever asks is not running: pausing is possible only after the answer.
    fails(
        &mut w,
        Action::PauseAgent {
            run: "BAU-142".into(),
        },
        ActionError::NothingToDo,
    );
    fails(
        &mut w,
        Action::ResumeAgent {
            run: "BAU-142".into(),
        },
        ActionError::NothingToDo,
    );
}

#[test]
fn steering_is_acknowledged_by_the_agent_in_the_next_step() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-150".into(),
            options: direct(),
        },
        1,
    );
    fails(
        &mut w,
        Action::SteerAgent {
            run: "BAU-150".into(),
            text: "".into(),
        },
        ActionError::InvalidInput,
    );
    ok(
        &mut w,
        Action::SteerAgent {
            run: "BAU-150".into(),
            text: "Bitte auch die Doku".into(),
        },
        1,
    );
    // The tick logs the acknowledged hint and the step.
    let tick = w.apply(Action::AdvanceAgents, Actor::Agent, T).unwrap();
    assert_eq!(tick.logged, 2);
    assert_eq!(tick.summary, "BAU-150: 1 Hinweis berücksichtigt");
    let run = w.run("BAU-150").unwrap();
    assert!(
        run.messages
            .iter()
            .any(|m| m.text == "Hinweis erhalten: Bitte auch die Doku")
    );
}
