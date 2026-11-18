//! Reading the catalog files and filling parameters.

use std::collections::{BTreeSet, HashMap};

use crate::{Arg, Lang};

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

pub(crate) fn fill(raw: &str, args: &[Arg]) -> String {
    let mut out = raw.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}
