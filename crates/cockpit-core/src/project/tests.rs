use super::*;
use crate::model::TicketStatus;

fn options(names: &[&str]) -> Vec<SelectOption> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| SelectOption {
            id: format!("o{i}"),
            name: (*n).into(),
        })
        .collect()
}

#[test]
fn project_refs_read_owner_and_number() {
    assert_eq!(
        ProjectRef::parse(" acme/3 "),
        Some(ProjectRef::new("acme", 3))
    );
    assert_eq!(ProjectRef::parse("acme#4"), Some(ProjectRef::new("acme", 4)));
    for bad in ["", "acme", "acme/0", "/3", "ac me/3", "acme/x"] {
        assert_eq!(ProjectRef::parse(bad), None, "{bad}");
    }
    assert_eq!(ProjectRef::new("acme", 3).to_string(), "acme/3");
}

#[test]
fn keys_have_a_long_and_a_short_form() {
    assert_eq!(issue_key("acme/app", 7), "acme/app#7");
    assert_eq!(short_key("acme/app#7"), "#7");
    assert_eq!(short_key("BAU-142"), "BAU-142");
    assert_eq!(short_key("acme/3#draft-ab12"), "acme/3#draft-ab12");
    assert_eq!(parse_issue_key("acme/app#7"), Some(("acme/app".into(), 7)));
    assert_eq!(parse_issue_key("a/b/c#7"), None);
    assert_eq!(parse_issue_key("acme/app#x"), None);
}

#[test]
fn a_draft_key_is_stable_and_has_no_number() {
    let key = draft_key("acme/3", "PVTI_lADOABCdef123456");
    assert_eq!(key, "acme/3#draft-123456");
    assert_eq!(key, draft_key("acme/3", "PVTI_lADOABCdef123456"));
    assert_eq!(parse_issue_key(&key), None);
}

fn keys(text: &str) -> Vec<(String, bool)> {
    issue_refs(text, "acme/app")
        .into_iter()
        .map(|r| (r.key(), r.closing))
        .collect()
}

#[test]
fn short_qualified_and_closing_references_are_found() {
    assert_eq!(keys("Closes #12"), [("acme/app#12".into(), true)]);
    assert_eq!(keys("fixes: #12"), [("acme/app#12".into(), true)]);
    assert_eq!(keys("Resolved acme/web#3"), [("acme/web#3".into(), true)]);
    assert_eq!(keys("see #4 and acme/web#5"), [
        ("acme/app#4".into(), false),
        ("acme/web#5".into(), false)
    ]);
    assert_eq!(
        keys("https://github.com/acme/web/issues/9"),
        [("acme/web#9".into(), false)]
    );
}

#[test]
fn the_same_issue_counts_once_and_closing_wins() {
    assert_eq!(keys("#7 and Fixes #7"), [("acme/app#7".into(), true)]);
}

#[test]
fn things_that_only_look_like_references_are_ignored() {
    assert!(keys("BAU-142").is_empty());
    assert!(keys("abc#12").is_empty());
    assert!(keys("color #fff and #12ab").is_empty());
    assert!(keys("fixes#1").iter().all(|(_, closing)| !closing));
    // Without a default repo a short reference has nowhere to point.
    assert!(issue_refs("see #4", "").is_empty());
}

#[test]
fn status_names_map_by_meaning() {
    for (name, want) in [
        ("Todo", TicketStatus::Open),
        ("Backlog", TicketStatus::Open),
        ("In Progress", TicketStatus::InProgress),
        ("Doing", TicketStatus::InProgress),
        ("In review", TicketStatus::Review),
        ("Done", TicketStatus::Done),
        ("Closed", TicketStatus::Done),
        ("Erledigt", TicketStatus::Done),
    ] {
        assert_eq!(classify(name), Some(want), "{name}");
    }
    assert_eq!(classify("Parked"), None);
}

#[test]
fn unknown_columns_take_their_place_in_the_row() {
    let o = options(&["Ideas", "Cooking", "Tasting", "Served"]);
    let mapped: Vec<_> = map_options(&o).into_iter().map(|(_, s)| s).collect();
    assert_eq!(
        mapped,
        [
            TicketStatus::Open,
            TicketStatus::InProgress,
            TicketStatus::InProgress,
            TicketStatus::Done
        ]
    );
}

#[test]
fn the_option_of_an_item_gives_the_status_and_back() {
    let o = options(&["Todo", "In Progress", "In Review", "Done"]);
    assert_eq!(status_of(&o, Some("o2")), TicketStatus::Review);
    assert_eq!(status_of(&o, None), TicketStatus::Open);
    assert_eq!(status_of(&o, Some("gone")), TicketStatus::Open);
    assert_eq!(option_for(&o, TicketStatus::Done).unwrap().id, "o3");
    // No review column: nothing to move to.
    let two = options(&["Todo", "Done"]);
    assert!(option_for(&two, TicketStatus::Review).is_none());
}

#[test]
fn a_status_field_without_the_default_columns_says_which_are_missing() {
    assert!(missing_columns(&options(&["Todo", "In Progress", "In Review", "Done"])).is_empty());
    assert_eq!(
        missing_columns(&options(&["Todo", "Done"])),
        ["In Progress", "In Review"]
    );
}

#[test]
fn the_status_field_is_the_chosen_one_or_status() {
    let meta = ProjectMeta {
        id: "PVT_1".into(),
        owner: "acme".into(),
        number: 3,
        title: "Board".into(),
        url: String::new(),
        fields: vec![
            FieldDef {
                id: "f1".into(),
                name: "Status".into(),
                kind: FieldKind::SingleSelect,
                options: options(&["Todo"]),
            },
            FieldDef {
                id: "f2".into(),
                name: "Stage".into(),
                kind: FieldKind::SingleSelect,
                options: options(&["A"]),
            },
            FieldDef {
                id: "f3".into(),
                name: "Points".into(),
                kind: FieldKind::Number,
                options: Vec::new(),
            },
        ],
        status_field: None,
    };
    assert_eq!(meta.status_def().unwrap().id, "f1");
    let own = ProjectMeta {
        status_field: Some("stage".into()),
        ..meta.clone()
    };
    assert_eq!(own.status_def().unwrap().id, "f2");
    // A number field cannot be the status.
    let bad = ProjectMeta {
        status_field: Some("Points".into()),
        ..meta
    };
    assert!(bad.status_def().is_none());
}
