use cockpit_core::adf;
use feature_ui::*;
use leptos::prelude::*;
use serde_json::json;

use super::{Story, common::Section};

pub const STORY: Story = Story {
    id: "rich-text",
    title: "Jira-Text",
    group: "Beispiele",
    description: "Beschreibungen und Kommentare aus Jira (ADF) als echte Elemente: Listen, Code, Tabellen, Hinweisfelder, Links und Platzhalter für Anhänge. Unbekanntes wird als Text gezeigt, unsichere Links nie als Link.",
    api: "<RichText doc> · .ui-rich · .ui-rich__panel[data-kind] · .ui-rich__code · <CommentBubble rich>",
    render,
};

fn text(t: &str) -> serde_json::Value {
    json!({"type": "text", "text": t})
}

fn para(t: &str) -> serde_json::Value {
    json!({"type": "paragraph", "content": [text(t)]})
}

fn item(t: &str) -> serde_json::Value {
    json!({"type": "listItem", "content": [para(t)]})
}

fn sample() -> adf::Doc {
    let marked = |t: &str, m: serde_json::Value| json!({"type": "text", "text": t, "marks": [m]});
    let value = json!({"type": "doc", "version": 1, "content": [
        {"type": "heading", "attrs": {"level": 1}, "content": [text("Überschrift 1")]},
        {"type": "heading", "attrs": {"level": 3}, "content": [text("Überschrift 3")]},
        {"type": "paragraph", "content": [
            marked("fett", json!({"type": "strong"})), text(" · "),
            marked("kursiv", json!({"type": "em"})), text(" · "),
            marked("Code", json!({"type": "code"})), text(" · "),
            marked("durchgestrichen", json!({"type": "strike"})), text(" · "),
            marked("unterstrichen", json!({"type": "underline"})), text(" · "),
            marked("Link", json!({"type": "link", "attrs": {"href": "https://example.com"}})), text(" · "),
            marked("unsicherer Link", json!({"type": "link", "attrs": {"href": "javascript:alert(1)"}})),
            {"type": "hardBreak"},
            {"type": "mention", "attrs": {"id": "x", "text": "@Mira Keller"}}, text(" "),
            {"type": "emoji", "attrs": {"shortName": ":tada:", "text": "🎉"}}, text(" "),
            {"type": "status", "attrs": {"text": "In Arbeit", "color": "blue"}}, text(" "),
            {"type": "date", "attrs": {"timestamp": "1731110400000"}}, text(" "),
            {"type": "inlineCard", "attrs": {"url": "https://example.com/BAU-1"}},
        ]},
        {"type": "orderedList", "content": [
            item("Erster Schritt"),
            {"type": "listItem", "content": [para("Zweiter Schritt"),
                {"type": "bulletList", "content": [item("Unterpunkt A"), item("Unterpunkt B")]}]},
        ]},
        {"type": "codeBlock", "attrs": {"language": "rust"}, "content": [text("fn main() {\n    println!(\"Hallo\");\n}")]},
        {"type": "blockquote", "content": [para("Ein Zitat.")]},
        {"type": "rule"},
        {"type": "table", "content": [
            {"type": "tableRow", "content": [
                {"type": "tableHeader", "content": [para("Name")]},
                {"type": "tableHeader", "content": [para("Wert")]}]},
            {"type": "tableRow", "content": [
                {"type": "tableCell", "content": [para("Abstand")]},
                {"type": "tableCell", "content": [para("8 px")]}]},
        ]},
        {"type": "panel", "attrs": {"panelType": "info"}, "content": [para("Info-Feld")]},
        {"type": "panel", "attrs": {"panelType": "success"}, "content": [para("Erfolg-Feld")]},
        {"type": "panel", "attrs": {"panelType": "warning"}, "content": [para("Warn-Feld")]},
        {"type": "panel", "attrs": {"panelType": "error"}, "content": [para("Fehler-Feld")]},
        {"type": "mediaSingle", "content": [{"type": "media", "attrs": {"type": "file", "id": "1", "alt": "screenshot.png"}}]},
        {"type": "unbekannterKnoten", "content": [para("Unbekannter Knoten: nur sein Text.")]},
    ]});
    adf::parse(&value, "").expect("valid sample")
}

fn render() -> AnyView {
    let comment = adf::parse(
        &json!({"type": "doc", "version": 1, "content": [{"type": "paragraph", "content": [
            text("Danke, "),
            {"type": "text", "text": "sieht gut aus", "marks": [{"type": "strong"}]},
            text("."),
        ]}]}),
        "",
    )
    .expect("valid sample");
    view! {
        <Section title="Beschreibung" api="<RichText doc>">
            <Card>
                <CardBody>
                    <RichText doc=sample() />
                </CardBody>
            </Card>
        </Section>

        <Section title="Kommentar" api="<CommentBubble rich>">
            <VStack spacing="12px">
                <CommentBubble who="Lena" initials="LK" time="vor 12 Min." text="Danke, sieht gut aus." rich=comment />
                <CommentBubble who="Du" initials="AS" time="gerade eben" text="Reiner Text bleibt lesbar." own=true />
            </VStack>
        </Section>
    }
    .into_any()
}
