//! The fields and attachments of a Jira issue. Pure functions,
//! without network.
//!
//! The search asks for all fields with `expand=names`, so every custom field has a name. What
//! is empty is left out; ADF fields (acceptance criteria and the like) become formatted text.

use std::collections::HashMap;

use cockpit_core::{Attachment, DetailField, DetailValue, Details};
use serde_json::Value;

use super::dto::Issue;
use super::map::sprint_of;

/// Which fields to show and in which order (`[jira] hidden_fields` / `field_order`). A line is
/// the ID (`customfield_10042`) or the name (`Acceptance criteria`) of a field, in any case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldPrefs {
    pub hidden: Vec<String>,
    pub order: Vec<String>,
}

/// The longest text one field shows.
const MAX_TEXT: usize = 4000;

/// System fields that are shown as plain extra fields (after the standard ones).
const EXTRA_SYSTEM: &[&str] = &["environment", "resolution", "versions"];

/// Custom fields that only carry the tool's own bookkeeping.
const NOISE_NAMES: &[&str] = &["rank", "development", "issue color"];

/// The fields and attachments of an issue.
pub fn details_of(
    issue: &Issue,
    me: &str,
    sprint_field: Option<&str>,
    prefs: &FieldPrefs,
) -> Details {
    let f = &issue.fields;
    let names = &issue.names;
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_default();
    let mut out: Vec<DetailField> = Vec::new();
    let mut push = |id: &str, name: String, value: Option<DetailValue>| {
        if let Some(value) = value {
            out.push(DetailField {
                id: id.to_string(),
                name,
                value,
            });
        }
    };

    let story_points = story_points_field(names);
    let named = |v: &Value| value_of(v, me);

    push("status", name_of("status"), text(&f.status.name));
    push(
        "issuetype",
        name_of("issuetype"),
        f.issuetype.as_ref().and_then(|t| text(&t.name)),
    );
    push(
        "priority",
        name_of("priority"),
        f.priority.as_ref().and_then(|p| text(&p.name)),
    );
    push(
        "assignee",
        name_of("assignee"),
        f.assignee.as_ref().and_then(|u| text(&u.display_name)),
    );
    push(
        "reporter",
        name_of("reporter"),
        f.reporter.as_ref().and_then(|u| text(&u.display_name)),
    );
    push("labels", name_of("labels"), list(f.labels.clone()));
    push(
        "components",
        name_of("components"),
        f.extra.get("components").and_then(named),
    );
    push(
        "fixVersions",
        name_of("fixVersions"),
        f.extra.get("fixVersions").and_then(named),
    );
    push(
        "sprint",
        sprint_field.map(name_of).unwrap_or_default(),
        sprint_field
            .and_then(|id| f.extra.get(id))
            .and_then(sprint_of)
            .and_then(|s| text(&s.name)),
    );
    push(
        "story_points",
        story_points.as_deref().map(name_of).unwrap_or_default(),
        story_points
            .as_deref()
            .and_then(|id| f.extra.get(id))
            .and_then(named),
    );
    push(
        "duedate",
        name_of("duedate"),
        f.duedate.as_deref().and_then(day),
    );
    push(
        "created",
        name_of("created"),
        f.extra
            .get("created")
            .and_then(|v| v.as_str())
            .and_then(day),
    );
    push("updated", name_of("updated"), day(&f.updated));
    push(
        "parent",
        name_of("parent"),
        f.extra.get("parent").and_then(parent),
    );
    push(
        "issuelinks",
        name_of("issuelinks"),
        f.extra.get("issuelinks").and_then(links),
    );

    for id in EXTRA_SYSTEM {
        push(id, name_of(id), f.extra.get(*id).and_then(named));
    }
    let has_parent = f.extra.get("parent").is_some_and(|p| !p.is_null());
    let mut custom: Vec<(&String, String)> = f
        .extra
        .keys()
        .filter(|id| id.starts_with("customfield_"))
        .filter(|id| {
            Some(id.as_str()) != sprint_field && Some(id.as_str()) != story_points.as_deref()
        })
        .map(|id| (id, name_of(id)))
        .filter(|(_, n)| !NOISE_NAMES.contains(&n.to_lowercase().as_str()))
        // „Epic Link“ says what the parent already says.
        .filter(|(_, n)| !(has_parent && n.eq_ignore_ascii_case("epic link")))
        .collect();
    custom.sort_by(|a, b| {
        a.1.to_lowercase()
            .cmp(&b.1.to_lowercase())
            .then(a.0.cmp(b.0))
    });
    for (id, name) in custom {
        push(id, name, f.extra.get(id.as_str()).and_then(named));
    }

    let mut fields = arrange(out, prefs);
    for field in &mut fields {
        if let DetailValue::Text(t) = &mut field.value {
            *t = t.chars().take(MAX_TEXT).collect();
        }
    }
    Details {
        fields,
        attachments: attachments_of(f.extra.get("attachment")),
    }
}

/// Applies `hidden_fields` and `field_order`.
pub fn arrange(mut fields: Vec<DetailField>, prefs: &FieldPrefs) -> Vec<DetailField> {
    let is = |entry: &String, f: &DetailField| {
        let e = entry.trim();
        e.eq_ignore_ascii_case(&f.id) || (!f.name.is_empty() && e.eq_ignore_ascii_case(&f.name))
    };
    fields.retain(|f| !prefs.hidden.iter().any(|h| is(h, f)));
    let mut first = Vec::new();
    for entry in &prefs.order {
        while let Some(i) = fields.iter().position(|f| is(entry, f)) {
            first.push(fields.remove(i));
        }
    }
    first.extend(fields);
    first
}

/// The attachments of an issue (`fields.attachment`). An entry without a numeric ID is left out:
/// the ID goes into the address of the download.
pub fn attachments_of(value: Option<&Value>) -> Vec<Attachment> {
    value
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let id = a["id"]
                .as_str()
                .map(str::to_string)
                .or_else(|| a["id"].as_u64().map(|n| n.to_string()))?;
            if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            Some(Attachment {
                id,
                name: a["filename"].as_str().unwrap_or_default().to_string(),
                size: a["size"].as_u64().unwrap_or(0),
                mime: a["mimeType"].as_str().unwrap_or_default().to_string(),
                thumbnail: a["thumbnail"].as_str().is_some_and(|t| !t.is_empty()),
            })
        })
        .collect()
}

/// The ID of the story points field: Jira names it „Story Points“ or „Story point estimate“.
fn story_points_field(names: &HashMap<String, String>) -> Option<String> {
    let mut found: Vec<&String> = names
        .iter()
        .filter(|(id, n)| {
            id.starts_with("customfield_")
                && matches!(
                    n.to_lowercase().as_str(),
                    "story points" | "story point estimate"
                )
        })
        .map(|(id, _)| id)
        .collect();
    found.sort();
    found.first().map(|s| s.to_string())
}

fn text(s: &str) -> Option<DetailValue> {
    let s = s.trim();
    (!s.is_empty()).then(|| DetailValue::Text(s.to_string()))
}

fn list(items: Vec<String>) -> Option<DetailValue> {
    let items: Vec<String> = items
        .into_iter()
        .map(|i| i.trim().to_string())
        .filter(|i| !i.is_empty())
        .collect();
    (!items.is_empty()).then_some(DetailValue::List(items))
}

/// `2026-11-09T10:00:00.000+0100` → `2026-11-09`.
fn day(s: &str) -> Option<DetailValue> {
    let d: String = s.trim().chars().take(10).collect();
    let ok = d.len() == 10
        && d.char_indices().all(|(i, c)| {
            if matches!(i, 4 | 7) {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        });
    ok.then_some(DetailValue::Date(d))
}

/// The parent: `BAU-100 · Summary`.
fn parent(v: &Value) -> Option<DetailValue> {
    let key = v["key"].as_str().filter(|k| !k.is_empty())?;
    match v["fields"]["summary"].as_str().filter(|s| !s.is_empty()) {
        Some(s) => text(&format!("{key} · {s}")),
        None => text(key),
    }
}

/// Linked issues: `blocks BAU-12 · Summary`.
fn links(v: &Value) -> Option<DetailValue> {
    let items = v.as_array()?.iter().filter_map(|l| {
        let (side, how) = match (l.get("outwardIssue"), l.get("inwardIssue")) {
            (Some(i), _) if i.is_object() => (i, l["type"]["outward"].as_str()),
            (_, Some(i)) if i.is_object() => (i, l["type"]["inward"].as_str()),
            _ => return None,
        };
        let key = side["key"].as_str().filter(|k| !k.is_empty())?;
        let mut line = String::new();
        if let Some(h) = how.filter(|h| !h.is_empty()) {
            line.push_str(h);
            line.push(' ');
        }
        line.push_str(key);
        if let Some(s) = side["fields"]["summary"].as_str().filter(|s| !s.is_empty()) {
            line.push_str(" · ");
            line.push_str(s);
        }
        Some(line)
    });
    list(items.collect())
}

/// A field value of any shape as something to show; `None` if it is empty.
fn value_of(v: &Value, me: &str) -> Option<DetailValue> {
    match v {
        Value::Null | Value::Bool(_) => None,
        Value::String(s) => text(s),
        Value::Number(n) => text(&number(n)),
        Value::Object(o) if o.get("type").and_then(Value::as_str) == Some("doc") => {
            cockpit_core::adf::parse(v, me).map(DetailValue::Rich)
        }
        Value::Object(_) => one_line(v).and_then(|t| text(&t)),
        Value::Array(items) => {
            // A list of formatted text blocks is unusual; the parts are shown as lines of text.
            list(items.iter().filter_map(one_line).collect())
        }
    }
}

fn number(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
        _ => n.to_string(),
    }
}

/// An option, version, user, component … as a line: the first of `displayName`, `name`,
/// `value`, `summary`; options with a child (`value` + `child`) as `A / B`.
fn one_line(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(number(n)),
        Value::Object(o) => {
            let own = ["displayName", "name", "value", "summary"]
                .iter()
                .find_map(|k| {
                    o.get(*k)
                        .and_then(Value::as_str)
                        .filter(|s| !s.trim().is_empty())
                })?
                .trim()
                .to_string();
            match o.get("child").and_then(one_line) {
                Some(child) => Some(format!("{own} / {child}")),
                None => Some(own),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
