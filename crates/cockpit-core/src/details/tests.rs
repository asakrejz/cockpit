use super::*;
use crate::adf::{Block, parse};
use serde_json::json;

fn png(id: &str, name: &str) -> Attachment {
    Attachment {
        id: id.into(),
        name: name.into(),
        size: 10,
        mime: "image/png".into(),
        thumbnail: true,
    }
}

#[test]
fn browse_url_needs_a_site_and_a_real_key() {
    assert_eq!(
        browse_url("https://acme.atlassian.net", "BAU-142").as_deref(),
        Some("https://acme.atlassian.net/browse/BAU-142")
    );
    // A slash at the end of the site does not double up.
    assert_eq!(
        browse_url("https://acme.atlassian.net/", "BAU-1").as_deref(),
        Some("https://acme.atlassian.net/browse/BAU-1")
    );
    assert_eq!(browse_url("", "BAU-1"), None);
    assert_eq!(browse_url("javascript:alert(1)", "BAU-1"), None);
    assert_eq!(browse_url("ftp://acme.atlassian.net", "BAU-1"), None);
    // The key goes into the address: nothing but project key, hyphen and digits.
    assert_eq!(browse_url("https://acme.atlassian.net", "BAU-1/../x"), None);
    assert_eq!(browse_url("https://acme.atlassian.net", "#87"), None);
    assert_eq!(browse_url("https://acme.atlassian.net", "BAU-"), None);
}

#[test]
fn only_real_pictures_are_inline() {
    for mime in [
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "IMAGE/PNG; x=y",
    ] {
        assert!(is_inline_image(mime), "{mime}");
    }
    for mime in ["image/svg+xml", "text/html", "application/pdf", ""] {
        assert!(!is_inline_image(mime), "{mime}");
    }
}

#[test]
fn sizes_read_well() {
    assert_eq!(size_text(812), "812 B");
    assert_eq!(size_text(2048), "2 KB");
    assert_eq!(size_text(1_500_000), "1.4 MB");
}

#[test]
fn media_finds_its_attachment_by_file_name() {
    let adf = json!({"type": "doc", "version": 1, "content": [
        {"type": "mediaSingle", "content": [{"type": "media", "attrs": {"alt": "Screen.PNG"}}]},
        {"type": "mediaSingle", "content": [{"type": "media", "attrs": {"alt": "notes.pdf"}}]},
        {"type": "mediaSingle", "content": [{"type": "media", "attrs": {"alt": "unknown.png"}}]}]});
    let mut doc = parse(&adf, "").unwrap();
    let list = [
        png("10001", "screen.png"),
        Attachment {
            mime: "application/pdf".into(),
            ..png("10002", "notes.pdf")
        },
    ];
    link_media(&mut doc, &list);
    let ids: Vec<Option<&str>> = doc
        .iter()
        .map(|b| match b {
            Block::Media { attachment, .. } => attachment.as_deref(),
            other => panic!("{other:?}"),
        })
        .collect();
    // The picture is linked, the PDF and the unknown name stay placeholders.
    assert_eq!(ids, [Some("10001"), None, None]);
}

#[test]
fn media_inside_panels_and_lists_is_found_too() {
    let adf = json!({"type": "doc", "version": 1, "content": [
        {"type": "panel", "attrs": {"panelType": "info"}, "content": [
            {"type": "mediaSingle", "content": [{"type": "media", "attrs": {"alt": "a.png"}}]}]}]});
    let mut doc = parse(&adf, "").unwrap();
    link_media(&mut doc, &[png("7", "a.png")]);
    let Block::Panel { content, .. } = &doc[0] else {
        panic!()
    };
    assert!(matches!(&content[0], Block::Media { attachment: Some(id), .. } if id == "7"));
}

#[test]
fn old_data_without_details_still_reads() {
    // An entry as an older version stored it: no `url`, no `details`.
    let old = json!({
        "kind": "Ticket", "source": "Jira", "reason": "Assigned",
        "scope": "Mine", "owner": {"name": "Mira", "initials": "M"}
    });
    let item: crate::Item = serde_json::from_value(old).unwrap();
    assert!(item.url.is_none() && item.details.is_none());
    // And an empty value is not written back.
    let back = serde_json::to_value(&item).unwrap();
    assert!(back.get("url").is_none() && back.get("details").is_none());
}

#[test]
fn details_survive_the_wire() {
    let d = Details {
        fields: vec![
            DetailField {
                id: "labels".into(),
                name: "Labels".into(),
                value: DetailValue::List(vec!["a11y".into()]),
            },
            DetailField {
                id: "customfield_1".into(),
                name: "Acceptance criteria".into(),
                value: DetailValue::Rich(vec![Block::Rule]),
            },
        ],
        attachments: vec![png("5", "a.png")],
    };
    let text = serde_json::to_string(&d).unwrap();
    assert_eq!(serde_json::from_str::<Details>(&text).unwrap(), d);
    // Missing parts read as empty.
    assert!(serde_json::from_str::<Details>("{}").unwrap().is_empty());
}

#[test]
fn web_url_prefers_the_stored_address() {
    let mut ws = crate::demo::workspace();
    let ticket = ws.items.iter_mut().find(|i| i.key == "BAU-142").unwrap();
    // The sample ticket has an address; without one (no site known) there is no link.
    assert!(ticket.web_url().is_some());
    ticket.url = None;
    assert_eq!(ticket.web_url(), None, "no site known: no link");
    ticket.url = Some("https://acme.atlassian.net/browse/BAU-142".into());
    assert_eq!(
        ticket.web_url().as_deref(),
        Some("https://acme.atlassian.net/browse/BAU-142")
    );
    ticket.url = Some("javascript:alert(1)".into());
    assert_eq!(ticket.web_url(), None, "only http and https");

    let pr = ws.items.iter().find(|i| i.key == "#87").unwrap();
    assert!(
        pr.web_url()
            .is_some_and(|u| u.starts_with("https://github.com/") && u.ends_with("/pull/87"))
    );
}
