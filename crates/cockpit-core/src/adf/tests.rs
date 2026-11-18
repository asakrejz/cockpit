use serde_json::json;

use super::*;

fn doc(content: serde_json::Value) -> serde_json::Value {
    json!({"type": "doc", "version": 1, "content": content})
}

fn text(t: &str) -> serde_json::Value {
    json!({"type": "text", "text": t})
}

fn para(t: &str) -> serde_json::Value {
    json!({"type": "paragraph", "content": [text(t)]})
}

#[test]
fn nested_lists_keep_their_structure() {
    let adf = doc(json!([{"type": "bulletList", "content": [
        {"type": "listItem", "content": [para("außen"), {"type": "orderedList", "attrs": {"order": 3}, "content": [
            {"type": "listItem", "content": [para("innen")]}]}]},
        {"type": "listItem", "content": [para("zweiter")]}]}]));
    let parsed = parse(&adf, "").unwrap();
    let Block::BulletList { items } = &parsed[0] else {
        panic!("bullet list expected: {parsed:?}")
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].len(), 2);
    assert!(matches!(&items[0][1], Block::OrderedList { start: 3, items } if items.len() == 1));
}

#[test]
fn code_block_has_language_and_keeps_lines() {
    let adf = doc(json!([{"type": "codeBlock", "attrs": {"language": "rust"},
        "content": [text("fn main() {\n}")]}]));
    assert_eq!(
        parse(&adf, "").unwrap(),
        vec![Block::CodeBlock {
            language: Some("rust".into()),
            text: "fn main() {\n}".into()
        }]
    );
}

#[test]
fn table_with_header_row() {
    let adf = doc(json!([{"type": "table", "content": [
        {"type": "tableRow", "content": [
            {"type": "tableHeader", "content": [para("A")]},
            {"type": "tableHeader", "content": [para("B")]}]},
        {"type": "tableRow", "content": [
            {"type": "tableCell", "content": [para("1")]},
            {"type": "tableCell", "content": [para("2")]}]}]}]));
    let parsed = parse(&adf, "").unwrap();
    let Block::Table { rows } = &parsed[0] else {
        panic!()
    };
    assert_eq!(rows.len(), 2);
    assert!(rows[0].iter().all(|c| c.header));
    assert!(rows[1].iter().all(|c| !c.header));
    assert_eq!(plain_text(&parsed), "A\nB\n1\n2");
}

#[test]
fn unknown_nodes_become_their_text_never_an_error() {
    let adf = doc(json!([
        {"type": "futureWidget", "content": [para("im Widget")]},
        {"type": "mystery", "attrs": {"text": "nur Attribut"}},
        {"type": "paragraph", "content": [text("vor "),
            {"type": "newInline", "content": [text("mitten")]}, text(" nach")]},
        {"type": "mystery2"}
    ]));
    let parsed = parse(&adf, "").unwrap();
    assert_eq!(
        plain_text(&parsed),
        "im Widget\nnur Attribut\nvor mitten nach"
    );
}

#[test]
fn evil_links_are_dropped_but_the_text_stays() {
    let adf = doc(json!([{"type": "paragraph", "content": [
        {"type": "text", "text": "klick", "marks": [{"type": "link", "attrs": {"href": "javascript:alert(1)"}}]},
        {"type": "text", "text": "daten", "marks": [{"type": "link", "attrs": {"href": "data:text/html,x"}}]},
        {"type": "text", "text": "gut", "marks": [{"type": "link", "attrs": {"href": "https://example.com/a"}}, {"type": "strong"}]},
        {"type": "inlineCard", "attrs": {"url": "JAVASCRIPT:alert(1)"}},
        {"type": "inlineCard", "attrs": {"url": "https://example.com/card"}}]}]));
    let parsed = parse(&adf, "").unwrap();
    let Block::Paragraph { content } = &parsed[0] else {
        panic!()
    };
    let links: Vec<_> = content
        .iter()
        .filter_map(|i| match i {
            Inline::Text { marks, .. } => marks.link.clone(),
            Inline::InlineCard { url } => Some(url.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(links, ["https://example.com/a", "https://example.com/card"]);
    assert!(plain_text(&parsed).contains("klick"));
    assert!(plain_text(&parsed).contains("JAVASCRIPT:alert(1)"));
}

#[test]
fn marks_and_inline_nodes() {
    let adf = doc(json!([{"type": "paragraph", "content": [
        {"type": "text", "text": "x", "marks": [{"type": "strong"}, {"type": "em"}, {"type": "code"},
            {"type": "strike"}, {"type": "underline"}, {"type": "textColor", "attrs": {}}]},
        {"type": "mention", "attrs": {"id": "acc-me", "text": "@Anna"}},
        {"type": "mention", "attrs": {"id": "acc-x", "text": "@Mira"}},
        {"type": "emoji", "attrs": {"shortName": ":tada:", "text": "🎉"}},
        {"type": "status", "attrs": {"text": "FERTIG", "color": "green"}},
        {"type": "date", "attrs": {"timestamp": "1731110400000"}},
        {"type": "hardBreak"}]}]));
    let parsed = parse(&adf, "acc-me").unwrap();
    let Block::Paragraph { content } = &parsed[0] else {
        panic!()
    };
    let Inline::Text { marks, .. } = &content[0] else {
        panic!()
    };
    assert!(marks.strong && marks.em && marks.code && marks.strike && marks.underline);
    assert_eq!(content[1], Inline::Mention { text: "@Du".into() });
    assert_eq!(
        content[2],
        Inline::Mention {
            text: "@Mira".into()
        }
    );
    assert_eq!(
        content[3],
        Inline::Emoji {
            text: "🎉".into()
        }
    );
    assert_eq!(
        content[5],
        Inline::Date {
            text: "2024-11-09".into()
        }
    );
    assert_eq!(content[6], Inline::HardBreak);
}

#[test]
fn heading_level_is_clamped_and_panel_and_quote_nest() {
    let adf = doc(json!([
        {"type": "heading", "attrs": {"level": 9}, "content": [text("H")]},
        {"type": "panel", "attrs": {"panelType": "warning"}, "content": [para("Achtung")]},
        {"type": "blockquote", "content": [para("Zitat")]},
        {"type": "rule"}]));
    let parsed = parse(&adf, "").unwrap();
    assert!(matches!(&parsed[0], Block::Heading { level: 6, .. }));
    assert!(matches!(
        &parsed[1],
        Block::Panel {
            kind: PanelKind::Warning,
            ..
        }
    ));
    assert!(matches!(&parsed[2], Block::Quote { .. }));
    assert_eq!(parsed[3], Block::Rule);
}

#[test]
fn media_is_a_placeholder_with_the_file_name() {
    let adf = doc(json!([{"type": "mediaSingle", "content": [
        {"type": "media", "attrs": {"type": "file", "id": "x", "alt": "bild.png"}}]}]));
    assert_eq!(
        parse(&adf, "").unwrap(),
        vec![Block::Media {
            name: "bild.png".into()
        }]
    );
}

#[test]
fn no_document_gives_none() {
    assert!(parse(&json!(null), "").is_none());
    assert!(parse(&json!("alter Text"), "").is_none());
    assert!(parse(&doc(json!([])), "").is_none());
}

#[test]
fn very_deep_nesting_does_not_overflow() {
    let mut node = para("tief");
    for _ in 0..100 {
        node = json!({"type": "blockquote", "content": [node]});
    }
    let parsed = parse(&doc(json!([node])), "").unwrap();
    assert!(plain_text(&parsed).contains("tief"));
}

#[test]
fn model_survives_a_json_round_trip() {
    let adf = doc(json!([para("a"), {"type": "orderedList", "content": [
        {"type": "listItem", "content": [para("b")]}]}]));
    let parsed = parse(&adf, "").unwrap();
    let back: Doc = serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
    assert_eq!(back, parsed);
}
