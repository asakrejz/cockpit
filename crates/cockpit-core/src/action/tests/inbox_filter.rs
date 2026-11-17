//! The filter dialog: applying, persisting, active filters and the person filter.

use super::*;
use crate::filters::{ASSIGNED_TO_ME, JIRA_PROJECT_FILTER, UNASSIGNED, UNASSIGNED_NAME};
use crate::{InboxFilter, Ownership, SourceFilter, SprintView};

fn narrowed() -> InboxFilter {
    InboxFilter {
        ownership: Ownership::All,
        sprint: SprintView::AllOpen,
        source: SourceFilter::Jira,
        person: Some(ASSIGNED_TO_ME.into()),
        ..Default::default()
    }
}

#[test]
fn applying_the_filter_is_stored_and_logged() {
    let mut w = ws();
    assert!(w.inbox_filter.is_default());
    ok(&mut w, Action::SetInboxFilter { filter: narrowed() }, 1);
    assert_eq!(w.inbox_filter, narrowed());
    assert_eq!(w.log.last().unwrap().text, "Filter angewendet");
    // The same filter again changes nothing.
    fails(
        &mut w,
        Action::SetInboxFilter { filter: narrowed() },
        ActionError::Unchanged,
    );
    ok(
        &mut w,
        Action::SetInboxFilter {
            filter: InboxFilter::default(),
        },
        1,
    );
    assert_eq!(w.log.last().unwrap().text, "Filter zurückgesetzt");
}

#[test]
fn the_filter_survives_saving_and_old_data_without_it_loads() {
    let mut w = ws();
    ok(&mut w, Action::SetInboxFilter { filter: narrowed() }, 1);
    let back = persist::load(&persist::save(&w)).unwrap();
    assert_eq!(back.inbox_filter, narrowed());

    let mut json: serde_json::Value = serde_json::from_str(&persist::save(&w)).unwrap();
    json["workspace"]
        .as_object_mut()
        .unwrap()
        .remove("inbox_filter");
    let old = persist::load(&json.to_string()).unwrap();
    assert!(old.inbox_filter.is_default());
    // A stored filter that lacks newer fields keeps the start values for them.
    json["workspace"]["inbox_filter"] = serde_json::json!({ "ownership": "All" });
    let partial = persist::load(&json.to_string()).unwrap();
    assert_eq!(partial.inbox_filter.ownership, Ownership::All);
    assert!(partial.inbox_filter.hide_done);
}

#[test]
fn a_demo_reset_keeps_the_chosen_filter() {
    let mut w = ws();
    ok(&mut w, Action::SetInboxFilter { filter: narrowed() }, 1);
    w.apply(Action::ResetDemo, Actor::User, T).unwrap();
    assert_eq!(w.inbox_filter, narrowed());
}

#[test]
fn active_filters_name_what_is_not_at_its_start_value() {
    let mut w = ws();
    assert!(w.active_filters().is_empty());
    ok(
        &mut w,
        Action::SelectJiraProject {
            project: Some("BAU".into()),
        },
        1,
    );
    ok(&mut w, Action::SetInboxFilter { filter: narrowed() }, 1);
    let active = w.active_filters();
    let ids: Vec<&str> = active.iter().map(|a| a.id).collect();
    assert_eq!(
        ids,
        [
            JIRA_PROJECT_FILTER,
            "source",
            "ownership",
            "sprint",
            "person"
        ]
    );
    let value = |id: &str| active.iter().find(|a| a.id == id).unwrap().value.clone();
    assert_eq!(value("person"), "Mir");
    assert_eq!(value("sprint"), "Alle offenen");
    assert_eq!(value(JIRA_PROJECT_FILTER), "BAU");
}

#[test]
fn removing_one_chip_resets_only_that_filter() {
    let mut by = narrowed();
    assert!(by.reset("person"));
    assert_eq!(by.person, None);
    assert_eq!(by.sprint, SprintView::AllOpen, "the others stay");
    assert!(!by.reset("nope"));
    let mut hidden = InboxFilter {
        hide_done: false,
        ..Default::default()
    };
    assert_eq!(hidden.active(), ["done"]);
    assert!(hidden.reset("done"));
    assert!(hidden.is_default());
}

#[test]
fn the_person_choices_are_me_nobody_and_the_people_of_the_tickets() {
    let w = ws();
    let choices = w.filter_choices("person");
    assert_eq!(choices[0], (ASSIGNED_TO_ME.to_string(), "Mir".to_string()));
    assert_eq!(
        choices[1],
        (UNASSIGNED.to_string(), "Nicht zugewiesen".to_string())
    );
    assert!(choices.len() > 2, "the colleagues of the tickets follow");
    assert!(
        !choices
            .iter()
            .any(|(v, _)| v == "Du" || v == UNASSIGNED_NAME),
        "me and nobody are not listed twice"
    );
}

#[test]
fn the_person_filter_selects_by_assignee_without_writing_anything() {
    let mut w = ws();
    let mut nobody = w.item_by_key("BAU-150").unwrap().clone();
    nobody.id = "nobody".into();
    nobody.key = "BAU-999".into();
    nobody.owner = Person::new(UNASSIGNED_NAME, "–");
    w.items.push(nobody);
    let by = |person: &str| InboxFilter {
        ownership: Ownership::All,
        sprint: SprintView::AllOpen,
        person: Some(person.into()),
        ..Default::default()
    };
    let keys = |w: &Workspace, by: &InboxFilter| -> Vec<String> {
        w.inbox(Filter::All, SourceFilter::All, "", T, by)
            .iter()
            .map(|i| i.key.clone())
            .collect()
    };
    assert_eq!(keys(&w, &by(UNASSIGNED)), ["BAU-999"]);
    let mine = keys(&w, &by(ASSIGNED_TO_ME));
    assert!(!mine.is_empty() && !mine.contains(&"BAU-999".to_string()));
    assert!(
        mine.iter()
            .all(|k| w.item_by_key(k).unwrap().owner == Person::me())
    );
    let before = w.log.len();
    keys(&w, &by("Lena Koch"));
    assert_eq!(w.log.len(), before, "choosing a person is no action");
}
