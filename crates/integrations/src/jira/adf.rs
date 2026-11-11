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

/// Wie [`adf_to_text`]; Erwähnungen von `ich` (Konto-ID) erscheinen als `@Du`, damit die
/// Regeln für Erwähnungen im Cockpit greifen.
pub fn adf_to_text_fuer(wert: &Value, ich: &str) -> String {
    fn ersetze(n: &mut Value, ich: &str) {
        if n["type"] == "mention" && n["attrs"]["id"] == ich {
            n["attrs"]["text"] = Value::String("@Du".into());
        }
        if let Some(kinder) = n["content"].as_array_mut() {
            for k in kinder {
                ersetze(k, ich);
            }
        }
    }
    let mut kopie = wert.clone();
    ersetze(&mut kopie, ich);
    adf_to_text(&kopie)
}

/// Text → ADF-Dokument für einen Kommentar: ein Absatz je Zeile.
pub fn text_to_adf(text: &str) -> Value {
    let absaetze: Vec<Value> = text
        .lines()
        .filter(|z| !z.trim().is_empty())
        .map(|z| serde_json::json!({"type": "paragraph", "content": [{"type": "text", "text": z}]}))
        .collect();
    serde_json::json!({"type": "doc", "version": 1, "content": absaetze})
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
    fn eigene_erwaehnung_wird_zu_du() {
        let adf = json!({"type":"doc","version":1,"content":[{"type":"paragraph","content":[
            {"type":"mention","attrs":{"id":"ich","text":"@Anna Schmidt"}},
            {"type":"text","text":" und "},
            {"type":"mention","attrs":{"id":"x","text":"@Mira"}}]}]});
        assert_eq!(adf_to_text_fuer(&adf, "ich"), "@Du und @Mira");
    }

    #[test]
    fn text_wird_zu_adf_absaetzen() {
        let adf = text_to_adf("Eins\n\nZwei");
        assert_eq!(adf["content"].as_array().unwrap().len(), 2);
        assert_eq!(adf_to_text(&adf), "Eins\nZwei");
    }

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
