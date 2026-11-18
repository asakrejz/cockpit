//! Messages that only developers or the terminal see must be English.
//!
//! The scan walks all Rust sources (and the TypeScript runner) and looks at the
//! string literals of `expect`, `panic!`, `unreachable!`, `todo!`, the assert
//! messages, the log macros, `println!`/`eprintln!` and `console.*`. It fails
//! when one of them contains an umlaut or a typical German word. Texts that
//! the user sees in the app stay German and are not touched here.

use std::fs;
use std::path::{Path, PathBuf};

/// Calls whose string arguments are all developer messages.
const PLAIN_CALLS: &[&str] = &[
    ".expect(",
    ".expect_err(",
    "panic!(",
    "unreachable!(",
    "todo!(",
    "unimplemented!(",
    "tracing::error!(",
    "tracing::warn!(",
    "tracing::info!(",
    "tracing::debug!(",
    "tracing::trace!(",
    "println!(",
    "eprintln!(",
    "console.log(",
    "console.warn(",
    "console.error(",
    "console.info(",
    "console.debug(",
];

/// Assert macros: only the optional message (the last argument) is checked,
/// because the other arguments are test data. The number is the count of
/// arguments before the message.
const ASSERT_CALLS: &[(&str, usize)] = &[
    ("assert!(", 1),
    ("debug_assert!(", 1),
    ("assert_eq!(", 2),
    ("assert_ne!(", 2),
    ("debug_assert_eq!(", 2),
    ("debug_assert_ne!(", 2),
];

/// Typical German words that do not occur in English messages.
const GERMAN_WORDS: &[&str] = &[
    "nicht",
    "kein",
    "keine",
    "keiner",
    "und",
    "oder",
    "wurde",
    "wurden",
    "fehlt",
    "fehler",
    "ist",
    "sind",
    "wird",
    "ohne",
    "noch",
    "nur",
    "für",
    "von",
    "mit",
    "nach",
    "beim",
    "darf",
    "soll",
    "erwartet",
    "gefunden",
    "gestartet",
    "datei",
    "aber",
    "wenn",
    "dann",
    "dass",
    "kann",
    "auch",
    "nichts",
    "eine",
    "einen",
    "kam",
    "ende",
    "falsche",
    "gesetzt",
    "geladen",
    "aufgerufen",
    "kopiert",
    "ereignis",
    "strom",
    "sperre",
    "zeitüberschreitung",
];

/// Existing exceptions as `(path suffix, literal)`. Keep empty if possible.
const ALLOWLIST: &[(&str, &str)] = &[];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !matches!(
                name.as_str(),
                "target" | "node_modules" | ".git" | "dist" | "config" | "fixtures"
            ) {
                collect(&p, out);
            }
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("rs" | "ts")) {
            out.push(p);
        }
    }
}

/// One argument of a call: the literal strings found in it, and whether it is
/// made of a single string literal only.
struct Arg {
    literals: Vec<String>,
    only_literal: bool,
}

/// Parses the arguments of the call whose `(` is right before `start`.
fn parse_args(src: &[char], start: usize) -> Vec<Arg> {
    let mut args = Vec::new();
    let mut cur = Arg {
        literals: vec![],
        only_literal: true,
    };
    let mut depth = 0usize;
    let mut other = false; // non-literal tokens seen in the current argument
    let mut i = start;
    while i < src.len() {
        let c = src[i];
        match c {
            '"' => {
                let mut s = String::new();
                i += 1;
                while i < src.len() && src[i] != '"' {
                    if src[i] == '\\' && i + 1 < src.len() {
                        i += 1;
                    }
                    s.push(src[i]);
                    i += 1;
                }
                cur.literals.push(s);
            }
            '\'' if i + 2 < src.len() && src[i + 2] == '\'' => i += 2, // char literal like '"'
            '/' if src.get(i + 1) == Some(&'/') => {
                while i < src.len() && src[i] != '\n' {
                    i += 1;
                }
            }
            '(' | '[' | '{' => {
                depth += 1;
                other = true;
            }
            ')' | ']' | '}' => {
                if depth == 0 {
                    cur.only_literal = !other;
                    args.push(cur);
                    return args;
                }
                depth -= 1;
            }
            ',' if depth == 0 => {
                cur.only_literal = !other;
                args.push(cur);
                cur = Arg {
                    literals: vec![],
                    only_literal: true,
                };
                other = false;
            }
            c if !c.is_whitespace() => other = true,
            _ => {}
        }
        i += 1;
    }
    args
}

/// Returns why a literal looks German, if it does.
fn german_reason(text: &str) -> Option<String> {
    if let Some(c) = text.chars().find(|c| "äöüßÄÖÜ".contains(*c)) {
        return Some(format!("umlaut `{c}`"));
    }
    let lower = text.to_lowercase();
    lower
        .split(|c: char| !c.is_alphabetic())
        .find(|w| GERMAN_WORDS.contains(w))
        .map(|w| format!("German word `{w}`"))
}

fn is_comment_line(src: &[char], at: usize) -> bool {
    let mut s = at;
    while s > 0 && src[s - 1] != '\n' {
        s -= 1;
    }
    src[s..at]
        .iter()
        .collect::<String>()
        .trim_start()
        .starts_with("//")
}

/// All findings in one source text, as `(literal, reason)`.
fn scan(text: &str) -> Vec<(String, String)> {
    let src: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    for (i, _) in src.iter().enumerate() {
        let rest: String = src[i..(i + 24).min(src.len())].iter().collect();
        let mut check = |literals: &[String]| {
            for l in literals {
                if let Some(r) = german_reason(l) {
                    found.push((l.clone(), r));
                }
            }
        };
        if let Some(call) = PLAIN_CALLS.iter().find(|c| rest.starts_with(**c)) {
            if is_comment_line(&src, i) {
                continue;
            }
            for a in parse_args(&src, i + call.len()) {
                check(&a.literals);
            }
        } else if let Some((call, before)) = ASSERT_CALLS.iter().find(|(c, _)| rest.starts_with(*c))
        {
            if is_comment_line(&src, i)
                || (i > 0 && (src[i - 1].is_alphanumeric() || src[i - 1] == '_'))
            {
                continue;
            }
            let args = parse_args(&src, i + call.len());
            let message = args
                .last()
                .filter(|a| args.len() > *before && a.only_literal);
            if let Some(msg) = message {
                check(&msg.literals);
            }
        }
    }
    found
}

#[test]
fn developer_messages_are_english() {
    let mut files = Vec::new();
    collect(&root().join("apps"), &mut files);
    collect(&root().join("crates"), &mut files);
    let me = Path::new(file!()).file_name().unwrap().to_owned();
    let mut problems = Vec::new();
    for f in files {
        if f.file_name() == Some(&me) {
            continue;
        }
        let text = fs::read_to_string(&f).unwrap();
        let rel = f
            .strip_prefix(root())
            .unwrap()
            .to_string_lossy()
            .replace("../", "");
        for (lit, why) in scan(&text) {
            if ALLOWLIST.iter().any(|(p, l)| rel.ends_with(p) && lit == *l) {
                continue;
            }
            problems.push(format!("{rel}: \"{lit}\" ({why})"));
        }
    }
    assert!(
        problems.is_empty(),
        "German text in developer messages (use English, or add to the allowlist):\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_scan_finds_german_messages() {
    let bad = r#"
        x.lock().expect("Sperre vergiftet");
        tracing::warn!("Fehler beim Laden: {e}");
        assert!(a, "es geht nicht");
        assert_eq!(a, "Größe", "ist nicht gleich");
        panic!("falsche Art");
    "#;
    assert_eq!(scan(bad).len(), 5);
}

#[test]
fn the_scan_ignores_english_messages_and_test_data() {
    let ok = r#"
        x.lock().expect("lock poisoned");
        tracing::warn!("Could not read: {e}");
        assert_eq!(title, "Größe ändern");
        assert!(text.contains("nicht erreichbar"));
        assert!(a, "no mention without @");
        // x.expect("Fehler")
    "#;
    assert_eq!(scan(ok), vec![]);
}
