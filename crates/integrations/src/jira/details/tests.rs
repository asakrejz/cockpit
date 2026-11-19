use super::*;
use serde_json::json;

fn issue(fields: serde_json::Value, names: serde_json::Value) -> Issue {
    let mut base = json!({"summary": "S", "status": {"name": "In Progress"}, "updated": "2026-11-02T10:00:00.000+0100"});
    base.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    let names: HashMap<String, String> = serde_json::from_value(names).unwrap();
    let mut i: Issue = serde_json::from_value(json!({"key": "BAU-1", "fields": base})).unwrap();
    i.names = std::sync::Arc::new(names);
    i
}

fn find<'a>(d: &'a Details, id: &str) -> Option<&'a DetailField> {
    d.fields.iter().find(|f| f.id == id)
}

#[test]
fn standard_fields_come_in_order_and_empty_ones_are_gone() {
    let i = issue(
        json!({
            "issuetype": {"name": "Bug"},
            "priority": {"name": "High"},
            "assignee": {"accountId": "a", "displayName": "Mira Keller"},
            "labels": ["a11y", "tokens"],
            "components": [],
            "fixVersions": [{"name": "1.4"}],
            "duedate": "2026-11-09",
            "created": "2026-10-01T08:00:00.000+0100",
            "parent": {"key": "BAU-100", "fields": {"summary": "Epic"}},
            "issuelinks": [{"type": {"outward": "blocks", "inward": "is blocked by"},
                "outwardIssue": {"key": "BAU-12", "fields": {"summary": "Other"}}}]
        }),
        json!({"status": "Status"}),
    );
    let d = details_of(&i, "me", None, &FieldPrefs::default());
    let ids: Vec<&str> = d.fields.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "status",
            "issuetype",
            "priority",
            "assignee",
            "labels",
            "fixVersions",
            "duedate",
            "created",
            "updated",
            "parent",
            "issuelinks"
        ]
    );
    // No reporter, no components: hidden because they are empty.
    assert!(find(&d, "components").is_none() && find(&d, "reporter").is_none());
    assert_eq!(
        find(&d, "created").unwrap().value,
        DetailValue::Date("2026-10-01".into())
    );
    assert_eq!(
        find(&d, "parent").unwrap().value,
        DetailValue::Text("BAU-100 · Epic".into())
    );
    assert_eq!(
        find(&d, "issuelinks").unwrap().value,
        DetailValue::List(vec!["blocks BAU-12 · Other".into()])
    );
    assert_eq!(find(&d, "status").unwrap().name, "Status");
}

#[test]
fn custom_fields_get_their_name_and_empty_ones_are_hidden() {
    let adf = json!({"type": "doc", "version": 1, "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "Works with keyboard"}]}]});
    let i = issue(
        json!({
            "customfield_10100": adf,
            "customfield_10101": null,
            "customfield_10102": "",
            "customfield_10103": {"value": "Design", "child": {"value": "Tokens"}},
            "customfield_10104": [{"value": "A"}, {"value": "B"}],
            "customfield_10105": 5.0,
            "customfield_10106": "0|i0001:",
            "customfield_10107": true
        }),
        json!({
            "customfield_10100": "Acceptance criteria",
            "customfield_10101": "Empty one",
            "customfield_10102": "Empty text",
            "customfield_10103": "Area",
            "customfield_10104": "Teams",
            "customfield_10105": "Story Points",
            "customfield_10106": "Rank",
            "customfield_10107": "Flag"
        }),
    );
    let d = details_of(&i, "me", None, &FieldPrefs::default());
    let ac = find(&d, "customfield_10100").unwrap();
    assert_eq!(ac.name, "Acceptance criteria");
    assert!(matches!(&ac.value, DetailValue::Rich(doc) if !doc.is_empty()));
    assert!(find(&d, "customfield_10101").is_none());
    assert!(find(&d, "customfield_10102").is_none());
    assert_eq!(
        find(&d, "customfield_10103").unwrap().value,
        DetailValue::Text("Design / Tokens".into())
    );
    assert_eq!(
        find(&d, "customfield_10104").unwrap().value,
        DetailValue::List(vec!["A".into(), "B".into()])
    );
    // Story points sit with the standard fields, as a whole number.
    assert_eq!(
        find(&d, "story_points").unwrap().value,
        DetailValue::Text("5".into())
    );
    assert!(find(&d, "customfield_10105").is_none());
    // The sort rank is bookkeeping, a bare boolean says nothing.
    assert!(find(&d, "customfield_10106").is_none() && find(&d, "customfield_10107").is_none());
}

#[test]
fn the_sprint_field_shows_the_active_sprint() {
    let i = issue(
        json!({"customfield_10020": [
            {"name": "Sprint 6", "state": "closed"}, {"name": "Sprint 7", "state": "active"}]}),
        json!({"customfield_10020": "Sprint"}),
    );
    let d = details_of(&i, "me", Some("customfield_10020"), &FieldPrefs::default());
    assert_eq!(
        find(&d, "sprint").unwrap().value,
        DetailValue::Text("Sprint 7".into())
    );
    assert!(find(&d, "customfield_10020").is_none(), "not twice");
}

#[test]
fn hidden_and_ordered_fields_match_by_name_or_id() {
    let i = issue(
        json!({"labels": ["x"], "customfield_1": "one", "customfield_2": "two"}),
        json!({"customfield_1": "Alpha", "customfield_2": "Beta"}),
    );
    let prefs = FieldPrefs {
        hidden: vec!["LABELS".into(), "status".into()],
        order: vec!["beta".into(), "customfield_1".into()],
    };
    let d = details_of(&i, "me", None, &prefs);
    let ids: Vec<&str> = d.fields.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(&ids[..2], ["customfield_2", "customfield_1"]);
    assert!(find(&d, "labels").is_none() && find(&d, "status").is_none());
}

#[test]
fn attachments_keep_name_size_type_and_skip_odd_ids() {
    let value = json!([
        {"id": "10001", "filename": "screen.png", "size": 2048, "mimeType": "image/png", "thumbnail": "https://x/t"},
        {"id": "10002", "filename": "spec.pdf", "size": 9, "mimeType": "application/pdf"},
        {"id": "../etc", "filename": "evil", "size": 1, "mimeType": "x"},
        {"filename": "no id"}
    ]);
    let list = attachments_of(Some(&value));
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, "10001");
    assert!(list[0].thumbnail && list[0].is_image() && list[0].size == 2048);
    assert!(!list[1].thumbnail && !list[1].is_image());
    assert!(attachments_of(None).is_empty());
}
