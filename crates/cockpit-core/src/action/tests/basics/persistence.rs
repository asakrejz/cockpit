use super::*;

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
        Action::SetVerdict {
            review: 85.into(),
            verdict: Verdict::Approve,
        },
        Actor::User,
        T + 1,
    )
    .unwrap();
    let back = loaded(&saved(&w)).unwrap();
    assert_eq!(back.log, w.log);
    assert_eq!(back, w);
}

#[test]
fn old_saves_without_a_log_still_load() {
    let mut value: serde_json::Value = serde_json::from_str(&saved(&ws())).unwrap();
    value["workspace"].as_object_mut().unwrap().remove("log");
    let back = loaded(&value.to_string()).unwrap();
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
fn old_saves_without_rules_snoozes_or_due_dates_still_load() {
    let mut value: serde_json::Value = serde_json::from_str(&saved(&ws())).unwrap();
    let workspace = value["workspace"].as_object_mut().unwrap();
    workspace.remove("rules");
    workspace.remove("snoozed");
    for item in workspace["items"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("due");
        if let Some(pr) = item.get_mut("pr").and_then(|p| p.as_object_mut()) {
            pr.remove("changes_requested");
        }
    }
    let back = loaded(&value.to_string()).unwrap();
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
