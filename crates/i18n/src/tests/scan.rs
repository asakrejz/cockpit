//! Scan test: no visible text is written as a literal in the UI code. Texts belong in the
//! catalogs (`catalog/<lang>/*.txt`); the code only uses keys.

use std::fs;

use super::{rust_files, workspace_root};

/// Names and terms that stay as they are in every language.
const ALLOWED: &[&str] = &[
    "Cockpit",
    "Jira",
    "GitHub",
    "Git",
    "Claude",
    "Anthropic",
    "OpenAI",
    "LiteLLM",
    "OAuth",
    "Docker",
    "Podman",
    "Playground",
    "Pull Request",
    "Rust",
    "Leptos",
    "Escape",
    "Tab",
    "Enter",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
];

/// Attribute names and call starts whose literal is never shown to people.
const TECHNICAL_BEFORE: &[&str] = &[
    "class=",
    "class:",
    "style=",
    "href=",
    "id=",
    "type=",
    "role=",
    "name=",
    "for=",
    "rel=",
    "target=",
    "d=",
    "viewBox=",
    "fill=",
    "stroke=",
    "key=",
    "inputmode=",
    "autocomplete=",
    "aria-current=",
    "aria-hidden=",
    "aria-live=",
    "aria-expanded=",
    "aria-selected=",
    "aria-pressed=",
    "aria-checked=",
    "aria-modal=",
    "aria-haspopup=",
    "aria-controls=",
    "aria-orientation=",
    "data-",
    "prop:",
    "attr:",
    "set_attribute(",
    "get_attribute(",
    "add_class(",
    "remove_class(",
    "toggle_class(",
    "query_selector(",
    "get_element_by_id(",
    "add_event_listener_with_callback(",
    "expect(",
    "panic!(",
    "unwrap_or(",
    "tag=",
    "mode=",
    "variant=",
    "icon=",
    "value=",
    "accept=",
    "pattern=",
    "enterkeyhint=",
    "dir=",
    "lang=",
    "width=",
    "height=",
    "xmlns=",
    "points=",
    "cx=",
    "cy=",
    "r=",
    "x=",
    "y=",
    "stroke-width=",
    "stroke-linecap=",
    "stroke-linejoin=",
    "unsafe(",
    "env!(",
    "concat!(",
    "include_str!(",
    "assert",
    "debug_assert",
    "log::",
    "console::",
    "eprintln!(",
    "println!(",
    "web_sys::console",
    "Url::",
    "format!(\"#",
    "format!(\"/",
    "format!(\"--",
];

fn is_css_like(text: &str) -> bool {
    text.split_whitespace().all(|tok| {
        tok.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_:/#.%()[]=,".contains(c))
            && (tok.contains(['-', '_', ':', '/', '#', '.', '%', '(', '[', '=']) || tok.len() <= 3)
    })
}

fn looks_visible(text: &str) -> bool {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    if letters < 2 || ALLOWED.contains(&text.trim()) {
        return false;
    }
    if text.contains(['ä', 'ö', 'ü', 'Ä', 'Ö', 'Ü', 'ß', '…', '„', '“', '·', '€']) {
        return true;
    }
    if text.starts_with("--")
        || text.contains("px")
        || text.contains("%)")
        || text.contains("{}%")
        || text.contains("var(--")
        || text.contains("translate")
    {
        return false;
    }
    if text.contains("://") || text.starts_with(['/', '#', '.', '<']) || text.contains("::") {
        return false;
    }
    let words = text.split_whitespace().count();
    if words >= 2 {
        return !is_css_like(text);
    }
    // One word: visible if it starts with a capital and is longer than a few letters.
    text.chars().next().is_some_and(char::is_uppercase)
        && text
            .chars()
            .all(|c| c.is_alphabetic() || c == '.' || c == ':' || c == '!')
        && letters >= 3
}

/// All string literals of a source with the text in front of them on the same line.
fn literals(source: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (n, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("#[") {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '/' if chars.get(i + 1) == Some(&'/') => break,
                '\'' => {
                    // Char literal ('x', '\n') or lifetime: skip a quoted char.
                    if chars.get(i + 2) == Some(&'\'') {
                        i += 3;
                    } else if chars.get(i + 1) == Some(&'\\') && chars.get(i + 3) == Some(&'\'') {
                        i += 4;
                    } else {
                        i += 1;
                    }
                }
                '"' => {
                    let before: String = chars[..i].iter().collect();
                    let mut text = String::new();
                    i += 1;
                    while i < chars.len() && chars[i] != '"' {
                        if chars[i] == '\\' && i + 1 < chars.len() {
                            i += 1;
                        }
                        text.push(chars[i]);
                        i += 1;
                    }
                    i += 1;
                    out.push((n + 1, before, text));
                }
                _ => i += 1,
            }
        }
    }
    out
}

fn production_part(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap_or(source)
}

/// Violations in one source: `line: literal`.
fn violations(source: &str) -> Vec<String> {
    literals(production_part(source))
        .into_iter()
        .filter(|(_, before, text)| {
            let before = before.trim_end();
            !TECHNICAL_BEFORE.iter().any(|t| {
                before.ends_with(t)
                    || before.contains(t)
                        && before
                            .rsplit(t)
                            .next()
                            .is_some_and(|rest| !rest.contains(' ') && !rest.contains('>'))
            }) && looks_visible(text)
        })
        .map(|(n, _, text)| format!("{n}: {text:?}"))
        .collect()
}

#[test]
fn no_visible_text_literals_in_the_ui_code() {
    let mut found = Vec::new();
    for dir in ["apps/cockpit/src", "crates/ui/src"] {
        for file in rust_files(dir) {
            let rel = file
                .strip_prefix(workspace_root())
                .unwrap()
                .to_string_lossy()
                .to_string();
            if rel.ends_with("tests.rs") || rel.contains("/tests/") {
                continue;
            }
            let source = fs::read_to_string(&file).unwrap();
            for v in violations(&source) {
                found.push(format!("{rel}:{v}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "visible text belongs in crates/i18n/catalog, not in the code:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scan_finds_german_and_english_texts() {
    assert_eq!(violations("let a = \"Speichern\";").len(), 1);
    assert_eq!(
        violations("view! { <p>\"Nothing to do here\"</p> }").len(),
        1
    );
    assert_eq!(violations("view! { <a title=\"Zurück\"></a> }").len(), 1);
    assert_eq!(violations("let a = \"größer\";").len(), 1);
}

#[test]
fn the_scan_ignores_technical_literals() {
    assert!(violations("view! { <div class=\"ui-stack ck-row is-active\"></div> }").is_empty());
    assert!(violations("let a = \"Cockpit\"; let b = \"GitHub\";").is_empty());
    assert!(violations("let a = \"in-progress\"; let b = \"#/inbox\";").is_empty());
    assert!(violations("let a = 1; // \"Speichern\"").is_empty());
    assert!(violations("fn f() {}\n#[cfg(test)]\nmod tests { let a = \"Speichern\"; }").is_empty());
}
