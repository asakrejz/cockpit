use super::*;

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
        channel: Channel::Browser,
        on,
    };
    let applied = ok(&mut w, mention(false), 1);
    assert_eq!(
        first_text(&w, &applied),
        "Benachrichtigung „Erwähnung“ über Browser ausgeschaltet"
    );
    assert!(
        !w.notifications
            .enabled(NotifyReason::Mention, Channel::Browser)
    );
    assert_eq!(w.log.last().unwrap().target, Some(Target::Settings));
    fails(&mut w, mention(false), ActionError::Unchanged);
    let applied = ok(&mut w, mention(true), 1);
    assert!(first_text(&w, &applied).ends_with("eingeschaltet"));

    let quiet = QuietHours {
        enabled: true,
        from: 22 * 60,
        to: 7 * 60,
    };
    let applied = ok(&mut w, Action::SetQuietHours { quiet }, 1);
    assert_eq!(
        first_text(&w, &applied),
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
    assert_eq!(first_text(&w, &applied), "Ruhezeiten ausgeschaltet");
}

#[test]
fn notification_settings_survive_save_and_load() {
    let mut w = ws();
    w.notifications
        .set(NotifyReason::AgentFinished, Channel::Browser, false);
    let back = loaded(&saved(&w)).unwrap();
    assert!(
        !back
            .notifications
            .enabled(NotifyReason::AgentFinished, Channel::Browser)
    );
    // Older states without the field still load, with the default values.
    let mut json: serde_json::Value = serde_json::from_str(&saved(&w)).unwrap();
    json["workspace"]
        .as_object_mut()
        .unwrap()
        .remove("notifications");
    let old = loaded(&json.to_string()).unwrap();
    assert_eq!(old.notifications, NotifySettings::default());
}
