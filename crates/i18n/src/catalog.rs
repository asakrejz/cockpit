//! Reading the catalog files and filling parameters.

use std::collections::{BTreeSet, HashMap};

use crate::Lang;

pub(crate) fn table(lang: Lang) -> &'static HashMap<String, String> {
    &crate::tables()[&lang]
}

/// All lines `(id, text)` of one file; blank lines and `#` comments are skipped.
pub fn lines(content: &str) -> Vec<(String, String)> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(id, text)| (id.trim().to_string(), unescape(text.trim())))
        .collect()
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

pub(crate) fn parse_all(files: &[(&str, &str)]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (_, content) in files {
        out.extend(lines(content));
    }
    out
}

/// The names of the `{parameters}` in a text.
pub fn placeholders(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        out.insert(after[..end].to_string());
        rest = &after[end + 1..];
    }
    out
}

/// The plural forms a key id can have.
pub fn plural_forms() -> [&'static str; 3] {
    ["zero", "one", "other"]
}

/// The text for an id (as stored in a [`crate::Msg`]). Unknown ids come back as the id.
pub(crate) fn text_by_id(lang: Lang, id: &str, args: &[(String, String)]) -> String {
    let entry = |lang: Lang, id: &str| table(lang).get(id).cloned();
    let plural = table(Lang::En).contains_key(&format!("{id}.other"));
    let pick = |lang: Lang| -> Option<String> {
        if !plural {
            return entry(lang, id);
        }
        let n: u64 = args
            .iter()
            .find(|(k, _)| k == "n")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0);
        let form = if n == 0 && entry(lang, &format!("{id}.zero")).is_some() {
            "zero"
        } else if n == 1 {
            "one"
        } else {
            "other"
        };
        entry(lang, &format!("{id}.{form}"))
    };
    let raw = pick(lang)
        .or_else(|| pick(Lang::En))
        .unwrap_or_else(|| id.to_string());
    fill_strings(&raw, args)
}

fn fill_strings(raw: &str, args: &[(String, String)]) -> String {
    let mut out = raw.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}
