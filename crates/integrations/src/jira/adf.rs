//! Atlassian Document Format (ADF) → plain text. Jira v3 delivers descriptions and
//! comments in this JSON tree.

use serde_json::Value;

/// Turns an ADF tree into text. Unknown nodes are traversed, their text is
/// kept. `null` or another type yields an empty text; a plain string
/// (older API) is taken over unchanged.
pub fn adf_to_text(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    let mut out = String::new();
    node(value, &mut out);
    out.trim().to_string()
}

/// Like [`adf_to_text`]; mentions of `me` (account ID) appear as `@Du`, so that the
/// mention rules in the cockpit apply.
pub fn adf_to_text_for(value: &Value, me: &str) -> String {
    fn replace(n: &mut Value, me: &str) {
        if n["type"] == "mention" && n["attrs"]["id"] == me {
            n["attrs"]["text"] = Value::String("@Du".into());
        }
        if let Some(children) = n["content"].as_array_mut() {
            for k in children {
                replace(k, me);
            }
        }
    }
    let mut copy = value.clone();
    replace(&mut copy, me);
    adf_to_text(&copy)
}

/// Text → ADF document for a comment: one paragraph per line.
pub fn text_to_adf(text: &str) -> Value {
    let paragraphs: Vec<Value> = text
        .lines()
        .filter(|z| !z.trim().is_empty())
        .map(|z| serde_json::json!({"type": "paragraph", "content": [{"type": "text", "text": z}]}))
        .collect();
    serde_json::json!({"type": "doc", "version": 1, "content": paragraphs})
}

fn children(n: &Value, out: &mut String) {
    for k in n["content"].as_array().into_iter().flatten() {
        node(k, out);
    }
}

fn line_end(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn node(n: &Value, out: &mut String) {
    match n["type"].as_str().unwrap_or("") {
        "text" => out.push_str(n["text"].as_str().unwrap_or("")),
        "hardBreak" => out.push('\n'),
        "mention" | "emoji" => out.push_str(n["attrs"]["text"].as_str().unwrap_or("")),
        "inlineCard" => out.push_str(n["attrs"]["url"].as_str().unwrap_or("")),
        "bulletList" | "orderedList" => {
            line_end(out);
            let numbered = n["type"] == "orderedList";
            for (i, entry) in n["content"].as_array().into_iter().flatten().enumerate() {
                let mut line = String::new();
                children(entry, &mut line);
                if numbered {
                    out.push_str(&format!("{}. ", i + 1));
                } else {
                    out.push_str("• ");
                }
                out.push_str(line.trim_end());
                out.push('\n');
            }
        }
        "paragraph" | "heading" | "codeBlock" | "blockquote" | "rule" => {
            line_end(out);
            children(n, out);
            line_end(out);
        }
        _ => children(n, out),
    }
}

#[cfg(test)]
mod tests {
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
}
