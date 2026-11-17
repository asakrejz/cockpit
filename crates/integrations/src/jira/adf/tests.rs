use super::*;
use serde_json::json;

#[test]
fn own_mention_becomes_user() {
    let adf = json!({"type":"doc","version":1,"content":[{"type":"paragraph","content":[
        {"type":"mention","attrs":{"id":"ich","text":"@Anna Schmidt"}},
        {"type":"text","text":" und "},
        {"type":"mention","attrs":{"id":"x","text":"@Mira"}}]}]});
    assert_eq!(adf_to_text_for(&adf, "ich"), "@Du und @Mira");
}

#[test]
fn text_becomes_adf_paragraphs() {
    let adf = text_to_adf("Eins\n\nZwei");
    assert_eq!(adf["content"].as_array().unwrap().len(), 2);
    assert_eq!(adf_to_text(&adf), "Eins\nZwei");
}

#[test]
fn paragraphs_lists_and_mentions() {
    let adf = json!({"type":"doc","version":1,"content":[
        {"type":"paragraph","content":[
            {"type":"text","text":"Hallo "},
            {"type":"mention","attrs":{"id":"1","text":"@Mira"}},
            {"type":"hardBreak"},
            {"type":"text","text":"zweite Zeile","marks":[{"type":"strong"}]}]},
        {"type":"bulletList","content":[
            {"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"eins"}]}]},
            {"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"zwei"}]}]}]},
        {"type":"orderedList","content":[
            {"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]}]}]}
    ]});
    assert_eq!(
        adf_to_text(&adf),
        "Hallo @Mira\nzweite Zeile\n• eins\n• zwei\n1. a"
    );
}

#[test]
fn empty_and_text() {
    assert_eq!(adf_to_text(&Value::Null), "");
    assert_eq!(adf_to_text(&json!("schlichter Text")), "schlichter Text");
    assert_eq!(adf_to_text(&json!({"type":"doc","content":[]})), "");
}
