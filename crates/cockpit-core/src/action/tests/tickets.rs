use super::*;

#[test]
fn editing_a_ticket_changes_title_and_description() {
    let mut w = ws();
    let applied = ok(
        &mut w,
        edit("bau150", "  Sheet: Detents  ", "Neuer Text"),
        1,
    );
    assert_eq!(
        applied.summary,
        "Titel und Beschreibung von BAU-150 geändert"
    );
    let item = w.item("bau150").unwrap();
    assert_eq!(
        item.title, "Sheet: Detents",
        "title without whitespace at the edges"
    );
    assert_eq!(item.description, "Neuer Text");
    let entry = w.log.last().unwrap();
    assert_eq!(
        (entry.actor, &entry.target),
        (Actor::User, &Some(Target::Item("bau150".into())))
    );

    let applied = ok(&mut w, edit("bau150", "Sheet: Detents", "Anderer Text"), 1);
    assert_eq!(applied.summary, "Beschreibung von BAU-150 geändert");
    let applied = ok(&mut w, edit("bau150", "Sheet", "Anderer Text"), 1);
    assert_eq!(applied.summary, "Titel von BAU-150 geändert");
    // An empty description is allowed.
    ok(&mut w, edit("bau150", "Sheet", ""), 1);
    assert_eq!(w.item("bau150").unwrap().description, "");
}

#[test]
fn a_ticket_needs_a_title() {
    let mut w = ws();
    for title in ["", "   ", "\n\t"] {
        fails(
            &mut w,
            edit("bau150", title, "Text"),
            ActionError::InvalidInput,
        );
    }
    fails(
        &mut w,
        edit(
            "bau150",
            "Sheet: Detents mittel und groß",
            "Das Sheet soll zwei Höhen unterstützen (mittel, groß) und sich per Grabber zwischen ihnen ziehen lassen.",
        ),
        ActionError::Unchanged,
    );
    fails(&mut w, edit("pr87", "Neu", "Neu"), ActionError::NotATicket);
    fails(
        &mut w,
        edit("gibt-es-nicht", "Neu", ""),
        ActionError::UnknownItem("gibt-es-nicht".into()),
    );
}

#[test]
fn assigning_to_someone_else_ends_the_assigned_rule() {
    use crate::attention::Rule;
    let mut w = ws();
    let assigned = |w: &Workspace| {
        w.hits(w.item("bau150").unwrap(), T)
            .iter()
            .any(|h| h.rule == Rule::Assigned)
    };
    assert!(assigned(&w), "BAU-150 is assigned to me");
    assert!(w.visible(Filter::Mine).iter().any(|i| i.id == "bau150"));

    let applied = ok(&mut w, assign("bau150", lena()), 1);
    assert_eq!(applied.summary, "BAU-150 an Lena zugewiesen");
    let item = w.item("bau150").unwrap();
    assert_eq!((item.owner.clone(), item.scope), (lena(), Scope::Team));
    assert!(!assigned(&w), "the rule ends");
    assert!(!w.visible(Filter::Mine).iter().any(|i| i.id == "bau150"));
    assert!(w.visible(Filter::Team).iter().any(|i| i.id == "bau150"));
    // The load moves along too.
    let load = |w: &Workspace, name: &str| {
        crate::team::members(w)
            .into_iter()
            .find(|m| m.person.name == name)
            .map_or(0, |m| m.total())
    };
    assert_eq!(load(&w, "Lena"), 1);
}

#[test]
fn assigning_to_me_triggers_the_assigned_rule() {
    use crate::attention::Rule;
    let mut w = ws();
    ok(&mut w, assign("bau150", lena()), 1);
    // The due date has passed: without the overdue rule only the assignment remains.
    w.items.iter_mut().find(|i| i.id == "bau150").unwrap().due = None;
    w.mark_done("bau150");
    assert!(!w.needs_attention(w.item("bau150").unwrap(), T));

    let applied = ok(&mut w, assign("bau150", Person::me()), 1);
    assert_eq!(applied.summary, "BAU-150 dir zugewiesen");
    let item = w.item("bau150").unwrap();
    assert_eq!(
        (item.owner.clone(), item.scope),
        (Person::me(), Scope::Mine)
    );
    assert!(!item.read, "newly assigned means unread again");
    let hits = w.hits(item, T);
    assert_eq!(
        hits.first().map(|h| (h.rule, h.text.de())),
        Some((Rule::Assigned, "Dir zugewiesen".to_string()))
    );
    assert!(
        w.attention(Filter::Mine, T)
            .iter()
            .any(|i| i.id == "bau150")
    );
    // If the rule is switched off, the assignment does not count.
    w.set_rule(Rule::Assigned, false);
    assert!(!w.needs_attention(w.item("bau150").unwrap(), T));
}

#[test]
fn assignment_checks_person_and_kind() {
    let mut w = ws();
    fails(
        &mut w,
        assign("bau150", Person::new("Fremde", "FR")),
        ActionError::UnknownPerson("Fremde".into()),
    );
    // The agent gets tickets through its run, not through the assignment.
    fails(
        &mut w,
        assign("bau150", Person::agent()),
        ActionError::UnknownPerson("Agent".into()),
    );
    fails(
        &mut w,
        assign("bau150", Person::me()),
        ActionError::Unchanged,
    );
    fails(
        &mut w,
        assign("pr87", Person::me()),
        ActionError::NotATicket,
    );
    fails(
        &mut w,
        assign("nix", Person::me()),
        ActionError::UnknownItem("nix".into()),
    );
    let names: Vec<String> = w.people().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["Du", "Jonas", "Mira", "Lena"]);
}

#[test]
fn priority_is_set_logged_and_defaults_to_medium() {
    let mut w = ws();
    assert_eq!(w.item("bau150").unwrap().priority, Priority::Medium);
    let applied = ok(
        &mut w,
        Action::SetPriority {
            item: "bau150".into(),
            priority: Priority::Highest,
        },
        1,
    );
    assert_eq!(
        applied.summary,
        "Priorität von BAU-150 auf „Höchste“ gesetzt"
    );
    assert_eq!(w.item("bau150").unwrap().priority, Priority::Highest);
    let same = Action::SetPriority {
        item: "bau150".into(),
        priority: Priority::Highest,
    };
    fails(&mut w, same, ActionError::Unchanged);
    let pr = Action::SetPriority {
        item: "pr87".into(),
        priority: Priority::Low,
    };
    fails(&mut w, pr, ActionError::NotATicket);
}

#[test]
fn old_saves_without_the_new_fields_still_load() {
    let mut w = ws();
    w.apply(
        Action::SetPriority {
            item: "bau150".into(),
            priority: Priority::High,
        },
        Actor::User,
        T,
    )
    .unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&persist::save(&w)).unwrap();
    let ws_json = value.get_mut("workspace").unwrap().as_object_mut().unwrap();
    for key in ["unlinked", "pr_status_off", "status_seen"] {
        assert!(ws_json.remove(key).is_some(), "{key} is stored");
    }
    for item in ws_json["items"].as_array_mut().unwrap() {
        let item = item.as_object_mut().unwrap();
        item.remove("priority");
        if let Some(pr) = item.get_mut("pr").and_then(|p| p.as_object_mut()) {
            pr.remove("review_requested");
        }
    }
    let (back, restored) = persist::restore(Some(&value.to_string()));
    assert!(restored, "old state loads");
    assert_eq!(back.item("bau150").unwrap().priority, Priority::Medium);
    assert!(!back.pr_status_off && back.unlinked.is_empty());
    assert!(
        !back
            .item("bau138")
            .unwrap()
            .pr
            .as_ref()
            .unwrap()
            .review_requested
    );
}
