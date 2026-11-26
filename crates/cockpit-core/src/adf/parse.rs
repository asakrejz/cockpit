//! ADF JSON → [`Doc`]. Never fails: whatever is not understood becomes text.

use serde_json::Value;

use super::{Block, Cell, Doc, Inline, Marks, PanelKind};

/// Deepest nesting that is still parsed structurally; below that the text is flattened.
const MAX_DEPTH: usize = 24;

/// Parses an ADF document. `me` is the account ID of the user: mentions of it show as `@Du`.
///
/// Returns `None` if the value is no ADF document (`null`, a plain string)
/// or has no content – the caller then uses the plain text.
pub fn parse(value: &Value, me: &str) -> Option<Doc> {
    if value["type"] != "doc" {
        return None;
    }
    let doc = blocks(&value["content"], me, 0);
    (!doc.is_empty()).then_some(doc)
}

/// The address, if it is allowed for a link: only `http` and `https`.
///
/// ```
/// use cockpit_core::adf::safe_url;
/// assert!(safe_url("https://example.com/a").is_some());
/// assert!(safe_url("javascript:alert(1)").is_none());
/// assert!(safe_url("  JavaScript:alert(1)").is_none());
/// ```
pub fn safe_url(url: &str) -> Option<&str> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let ok = !rest.is_empty() && !url.chars().any(|c| c.is_control() || c.is_whitespace());
    ok.then_some(url)
}

fn kind_of(n: &Value) -> &str {
    n["type"].as_str().unwrap_or("")
}

fn list(n: &Value) -> impl Iterator<Item = &Value> {
    n["content"].as_array().into_iter().flatten()
}

/// Is the node an inline node?
fn is_inline(n: &Value) -> bool {
    matches!(
        kind_of(n),
        "text" | "hardBreak" | "mention" | "emoji" | "inlineCard" | "status" | "date"
    )
}

/// Blocks from a `content` list. Inline nodes between blocks (as in a `taskItem`) are
/// gathered into a paragraph.
fn blocks(content: &Value, me: &str, depth: usize) -> Vec<Block> {
    let mut out = Vec::new();
    let mut pending: Vec<&Value> = Vec::new();
    let flush = |pending: &mut Vec<&Value>, out: &mut Vec<Block>| {
        if !pending.is_empty() {
            let content = inlines(pending.drain(..), me);
            out.push(Block::Paragraph { content });
        }
    };
    for n in content.as_array().into_iter().flatten() {
        if is_inline(n) {
            pending.push(n);
        } else {
            flush(&mut pending, &mut out);
            out.extend(block(n, me, depth + 1));
        }
    }
    flush(&mut pending, &mut out);
    out
}

fn block(n: &Value, me: &str, depth: usize) -> Vec<Block> {
    if depth > MAX_DEPTH {
        return text_block(n);
    }
    match kind_of(n) {
        "paragraph" => vec![Block::Paragraph {
            content: inlines(list(n), me),
        }],
        "heading" => vec![Block::Heading {
            level: n["attrs"]["level"].as_u64().unwrap_or(1).clamp(1, 6) as u8,
            content: inlines(list(n), me),
        }],
        "bulletList" | "taskList" => vec![Block::BulletList {
            items: items(n, me, depth),
        }],
        "orderedList" => vec![Block::OrderedList {
            start: n["attrs"]["order"]
                .as_u64()
                .and_then(|o| u32::try_from(o).ok())
                .unwrap_or(1),
            items: items(n, me, depth),
        }],
        "codeBlock" => vec![Block::CodeBlock {
            language: n["attrs"]["language"]
                .as_str()
                .filter(|l| !l.is_empty() && *l != "text")
                .map(str::to_string),
            text: flat_text(n),
        }],
        "blockquote" => vec![Block::Quote {
            content: blocks(&n["content"], me, depth),
        }],
        "rule" => vec![Block::Rule],
        "table" => vec![Block::Table {
            rows: list(n)
                .filter(|r| kind_of(r) == "tableRow")
                .map(|r| {
                    list(r)
                        .map(|c| Cell {
                            header: kind_of(c) == "tableHeader",
                            content: blocks(&c["content"], me, depth),
                        })
                        .collect()
                })
                .collect(),
        }],
        "panel" => vec![Block::Panel {
            kind: match n["attrs"]["panelType"].as_str().unwrap_or("") {
                "note" => PanelKind::Note,
                "success" => PanelKind::Success,
                "warning" => PanelKind::Warning,
                "error" => PanelKind::Error,
                _ => PanelKind::Info,
            },
            content: blocks(&n["content"], me, depth),
        }],
        "media" => vec![Block::Media {
            name: media_name(n),
            attachment: None,
        }],
        // `mediaSingle`, `mediaGroup`, `expand`, layouts … and everything unknown:
        // the content counts, the container does not.
        _ if n["content"].is_array() => blocks(&n["content"], me, depth),
        _ => text_block(n),
    }
}

fn items(n: &Value, me: &str, depth: usize) -> Vec<Vec<Block>> {
    list(n)
        .map(|item| blocks(&item["content"], me, depth))
        .collect()
}

fn media_name(n: &Value) -> String {
    ["__fileName", "alt"]
        .iter()
        .find_map(|k| n["attrs"][k].as_str().filter(|s| !s.is_empty()))
        .unwrap_or("Anhang")
        .to_string()
}

/// Paragraph with the flat text of a node (empty if there is none).
fn text_block(n: &Value) -> Vec<Block> {
    let text = flat_text(n);
    if text.trim().is_empty() {
        return Vec::new();
    }
    vec![Block::Paragraph {
        content: vec![Inline::Text {
            text,
            marks: Marks::default(),
        }],
    }]
}

/// All text in a node and below it.
fn flat_text(n: &Value) -> String {
    let mut out = String::new();
    collect_text(n, &mut out, 0);
    out
}

fn collect_text(n: &Value, out: &mut String, depth: usize) {
    if depth > 256 {
        return;
    }
    match kind_of(n) {
        "text" => out.push_str(n["text"].as_str().unwrap_or("")),
        "hardBreak" => out.push('\n'),
        _ => {
            if let Some(t) = n["attrs"]["text"].as_str() {
                out.push_str(t);
            }
            for k in list(n) {
                collect_text(k, out, depth + 1);
            }
        }
    }
}

fn inlines<'a>(nodes: impl Iterator<Item = &'a Value>, me: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for n in nodes {
        match kind_of(n) {
            "text" => {
                let text = n["text"].as_str().unwrap_or("");
                if !text.is_empty() {
                    out.push(Inline::Text {
                        text: text.to_string(),
                        marks: marks(n),
                    });
                }
            }
            "hardBreak" => out.push(Inline::HardBreak),
            "mention" => {
                let text = if n["attrs"]["id"] == me && !me.is_empty() {
                    "@Du".to_string()
                } else {
                    n["attrs"]["text"].as_str().unwrap_or("@Jemand").to_string()
                };
                out.push(Inline::Mention { text });
            }
            "emoji" => {
                let text = n["attrs"]["text"]
                    .as_str()
                    .or_else(|| n["attrs"]["shortName"].as_str())
                    .unwrap_or("");
                if !text.is_empty() {
                    out.push(Inline::Emoji { text: text.into() });
                }
            }
            "inlineCard" => {
                if let Some(url) = n["attrs"]["url"].as_str() {
                    out.push(match safe_url(url) {
                        Some(url) => Inline::InlineCard { url: url.into() },
                        None => plain(url),
                    });
                }
            }
            "status" => out.push(Inline::Status {
                text: n["attrs"]["text"].as_str().unwrap_or("").to_string(),
                color: n["attrs"]["color"].as_str().unwrap_or("").to_string(),
            }),
            "date" => {
                if let Some(text) = date_of(&n["attrs"]["timestamp"]) {
                    out.push(Inline::Date { text });
                }
            }
            _ => {
                let text = flat_text(n);
                if !text.is_empty() {
                    out.push(plain(&text));
                }
            }
        }
    }
    out
}

fn plain(text: &str) -> Inline {
    Inline::Text {
        text: text.to_string(),
        marks: Marks::default(),
    }
}

fn marks(n: &Value) -> Marks {
    let mut m = Marks::default();
    for mark in n["marks"].as_array().into_iter().flatten() {
        match kind_of(mark) {
            "strong" => m.strong = true,
            "em" => m.em = true,
            "code" => m.code = true,
            "strike" => m.strike = true,
            "underline" => m.underline = true,
            "link" => {
                m.link = mark["attrs"]["href"]
                    .as_str()
                    .and_then(safe_url)
                    .map(str::to_string);
            }
            _ => {}
        }
    }
    m
}

/// `date` carries milliseconds since 1970 as a string; the result is the day (UTC).
fn date_of(timestamp: &Value) -> Option<String> {
    let ms: i64 = match timestamp {
        Value::String(s) => s.trim().parse().ok()?,
        Value::Number(n) => n.as_i64()?,
        _ => return None,
    };
    let days = ms.div_euclid(86_400_000);
    // Civil date from days since 1970 (algorithm by Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some(format!("{year:04}-{month:02}-{day:02}"))
}
