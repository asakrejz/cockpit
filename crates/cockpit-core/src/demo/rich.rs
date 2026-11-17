//! Formatted Jira text (ADF) for the sample data: shows lists, code, table and panel.

use serde_json::json;

use crate::adf::{self, Doc};

fn text(t: &str) -> serde_json::Value {
    json!({"type": "text", "text": t})
}

fn para(t: &str) -> serde_json::Value {
    json!({"type": "paragraph", "content": [text(t)]})
}

fn item(t: &str) -> serde_json::Value {
    json!({"type": "listItem", "content": [para(t)]})
}

fn cell(kind: &str, t: &str) -> serde_json::Value {
    json!({"type": kind, "content": [para(t)]})
}

/// The description of BAU-142 as Jira would deliver it.
pub(super) fn bau142_description() -> Doc {
    let adf = json!({"type": "doc", "version": 1, "content": [
        {"type": "paragraph", "content": [
            text("Im Dark Mode ist der "),
            {"type": "text", "text": "Fokus-Ring", "marks": [{"type": "strong"}]},
            text(" des Toggles kaum zu erkennen. Erwartet: Der Ring nutzt "),
            {"type": "text", "text": "--sys-focus-ring", "marks": [{"type": "code"}]},
            text(" und bleibt auf dunklem Hintergrund klar sichtbar."),
        ]},
        {"type": "heading", "attrs": {"level": 2}, "content": [text("Schritte")]},
        {"type": "orderedList", "content": [
            item("Dark Mode einschalten"),
            {"type": "listItem", "content": [para("Mit Tab zum Toggle gehen"),
                {"type": "bulletList", "content": [item("Ring ist kaum sichtbar"), item("Kontrast unter 3:1")]}]},
        ]},
        {"type": "codeBlock", "attrs": {"language": "css"},
            "content": [text(".toggle:focus-visible {\n  outline: 2px solid var(--sys-focus-ring);\n}")]},
        {"type": "table", "content": [
            {"type": "tableRow", "content": [cell("tableHeader", "Modus"), cell("tableHeader", "Kontrast")]},
            {"type": "tableRow", "content": [cell("tableCell", "Light"), cell("tableCell", "4,8:1")]},
            {"type": "tableRow", "content": [cell("tableCell", "Dark"), cell("tableCell", "1,9:1")]},
        ]},
        {"type": "panel", "attrs": {"panelType": "warning"}, "content": [
            para("Gilt auch für den Slider-Thumb."),
        ]},
        {"type": "paragraph", "content": [
            text("Mehr dazu in den "),
            {"type": "text", "text": "Design-Notizen", "marks": [{"type": "link", "attrs": {"href": "https://example.com/notes"}}]},
            text(". Anhang: "),
        ]},
        {"type": "mediaSingle", "content": [{"type": "media", "attrs": {"type": "file", "id": "demo", "alt": "fokus-ring.png"}}]},
    ]});
    adf::parse(&adf, "").expect("sample ADF is valid")
}

/// The comment of Lena on BAU-142.
pub(super) fn lena_comment() -> Doc {
    let adf = json!({"type": "doc", "version": 1, "content": [
        {"type": "paragraph", "content": [
            {"type": "mention", "attrs": {"id": "demo-lena-mention", "text": "@AS"}},
            text(" Betrifft auch den "),
            {"type": "text", "text": "Slider-Thumb", "marks": [{"type": "em"}]},
            text(". Kannst du das gleich mit prüfen?"),
        ]},
    ]});
    adf::parse(&adf, "").expect("sample ADF is valid")
}
