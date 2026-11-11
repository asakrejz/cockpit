//! Atlassian Document Format (ADF) → Klartext. Jira v3 liefert Beschreibungen und
//! Kommentare in diesem JSON-Baum.

use serde_json::Value;

/// Wandelt einen ADF-Baum in Text. Unbekannte Knoten werden durchlaufen, ihr Text bleibt
/// erhalten. `null` oder ein anderer Typ ergibt einen leeren Text; ein einfacher String
/// (ältere API) wird unverändert übernommen.
pub fn adf_to_text(wert: &Value) -> String {
    if let Some(s) = wert.as_str() {
        return s.to_string();
    }
    let mut out = String::new();
    knoten(wert, &mut out);
    out.trim().to_string()
}

fn kinder(n: &Value, out: &mut String) {
    for k in n["content"].as_array().into_iter().flatten() {
        knoten(k, out);
    }
}

fn zeilenende(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn knoten(n: &Value, out: &mut String) {
    match n["type"].as_str().unwrap_or("") {
        "text" => out.push_str(n["text"].as_str().unwrap_or("")),
        "hardBreak" => out.push('\n'),
        "mention" | "emoji" => out.push_str(n["attrs"]["text"].as_str().unwrap_or("")),
        "inlineCard" => out.push_str(n["attrs"]["url"].as_str().unwrap_or("")),
        "bulletList" | "orderedList" => {
            zeilenende(out);
            let nummeriert = n["type"] == "orderedList";
            for (i, eintrag) in n["content"].as_array().into_iter().flatten().enumerate() {
                let mut zeile = String::new();
                kinder(eintrag, &mut zeile);
                if nummeriert {
                    out.push_str(&format!("{}. ", i + 1));
                } else {
                    out.push_str("• ");
                }
                out.push_str(zeile.trim_end());
                out.push('\n');
            }
        }
        "paragraph" | "heading" | "codeBlock" | "blockquote" | "rule" => {
            zeilenende(out);
            kinder(n, out);
            zeilenende(out);
        }
        _ => kinder(n, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absaetze_listen_und_erwaehnungen() {
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
    fn leer_und_text() {
        assert_eq!(adf_to_text(&Value::Null), "");
        assert_eq!(adf_to_text(&json!("schlichter Text")), "schlichter Text");
        assert_eq!(adf_to_text(&json!({"type":"doc","content":[]})), "");
    }
}
