use std::{collections::BTreeSet, fs, path::PathBuf};

use super::*;

mod scan;
mod scan_core;

pub(crate) fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// All `.rs` files below `dir` (relative to the workspace root).
pub(crate) fn rust_files(dir: &str) -> Vec<PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&workspace_root().join(dir), &mut out);
    out.sort();
    out
}

#[test]
fn both_catalogs_have_the_same_files_keys_and_parameters() {
    let names = |files: &'static [(&'static str, &'static str)]| {
        files.iter().map(|f| f.0).collect::<BTreeSet<_>>()
    };
    assert_eq!(names(EN_FILES), names(DE_FILES), "catalog files differ");
    for ((name, en), (_, de)) in EN_FILES.iter().zip(DE_FILES.iter()) {
        let en = catalog::lines(en);
        let de = catalog::lines(de);
        let ids = |l: &[(String, String)]| l.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
        let (en_ids, de_ids) = (ids(&en), ids(&de));
        assert_eq!(
            en_ids.iter().collect::<BTreeSet<_>>().len(),
            en_ids.len(),
            "{name}: duplicate id in en"
        );
        assert_eq!(
            en_ids.iter().collect::<BTreeSet<_>>(),
            de_ids.iter().collect::<BTreeSet<_>>(),
            "{name}: ids of en and de differ"
        );
        for (id, text) in &en {
            let other = &de.iter().find(|(i, _)| i == id).unwrap().1;
            assert_eq!(
                placeholders(text),
                placeholders(other),
                "{name}: parameters of `{id}` differ"
            );
            assert!(
                !text.is_empty() && !other.is_empty(),
                "{name}: `{id}` is empty"
            );
        }
    }
}

#[test]
fn plural_keys_have_one_and_other_in_both_languages() {
    for lang in Lang::ALL {
        let table = catalog::table(lang);
        for key in Key::ALL {
            let id = key.id();
            let has = |form: &str| table.contains_key(&format!("{id}.{form}"));
            if table.contains_key(id) {
                assert!(!has("one") && !has("other"), "{id}: plain and plural");
            } else {
                assert!(
                    has("one") && has("other"),
                    "{id}: {lang:?} needs one and other"
                );
            }
        }
    }
}

#[test]
fn every_key_is_used_in_the_code() {
    let mut source = String::new();
    for dir in ["apps", "crates"] {
        for file in rust_files(dir) {
            let rel = file
                .strip_prefix(workspace_root())
                .unwrap()
                .to_string_lossy()
                .to_string();
            if rel.starts_with("crates/i18n/src/tests") || rel.starts_with("crates/i18n/build.rs") {
                continue;
            }
            source.push_str(&fs::read_to_string(&file).unwrap());
        }
    }
    let used: BTreeSet<&str> = source
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .collect();
    let unused: Vec<String> = Key::ALL
        .iter()
        .map(|k| format!("{k:?}"))
        .filter(|name| !used.contains(name.as_str()))
        .collect();
    assert!(unused.is_empty(), "unused catalog entries: {unused:#?}");
}

#[test]
fn texts_fill_parameters_and_pick_the_plural() {
    assert_eq!(
        text(Lang::En, Key::TimeAgoMinutes, &[("n", "1".into())]),
        "1 minute ago"
    );
    assert_eq!(
        text(Lang::En, Key::TimeAgoMinutes, &[("n", "5".into())]),
        "5 minutes ago"
    );
    assert_eq!(
        text(Lang::De, Key::TimeAgoMinutes, &[("n", "5".into())]),
        "vor 5 Minuten"
    );
    assert_eq!(
        text(Lang::De, Key::TimeAgoDays, &[("n", "1".into())]),
        "vor 1 Tag"
    );
}

#[test]
fn relative_time_per_language() {
    assert_eq!(ago(Lang::En, 30), "just now");
    assert_eq!(ago(Lang::De, 30), "gerade eben");
    assert_eq!(ago(Lang::En, 300), "5 minutes ago");
    assert_eq!(ago(Lang::De, 7200), "vor 2 Stunden");
    assert_eq!(ago_ms(Lang::En, 10_000_000, 1), "just now");
}

#[test]
fn language_from_the_browser() {
    assert_eq!(Lang::from_browser("de"), Lang::De);
    assert_eq!(Lang::from_browser("de-AT"), Lang::De);
    assert_eq!(Lang::from_browser("DE-ch"), Lang::De);
    assert_eq!(Lang::from_browser("en-US"), Lang::En);
    assert_eq!(Lang::from_browser("fr"), Lang::En);
    assert_eq!(Lang::from_browser(""), Lang::En);
}

#[test]
fn language_survives_the_wire() {
    assert_eq!(serde_json::to_string(&Lang::De).unwrap(), "\"de\"");
}

#[test]
fn messages_render_in_both_languages_and_nest() {
    let inner = msg!(TimeAgoMinutes, n = 5);
    let outer = msg!(TimeAgoDays, n = ArgValue::from(inner.clone()));
    assert_eq!(inner.render(Lang::En), "5 minutes ago");
    assert_eq!(inner.render(Lang::De), "vor 5 Minuten");
    assert_eq!(inner.de(), "vor 5 Minuten");
    // The nested text is filled into the outer one in the same language.
    assert_eq!(outer.render(Lang::De), "vor vor 5 Minuten Tagen");
    let joined = ArgValue::joined([inner.clone(), inner.clone()], "; ");
    let list = Msg::new(Key::TimeAgoDays, vec![("n", joined)]);
    assert_eq!(
        list.render(Lang::En),
        "5 minutes ago; 5 minutes ago days ago"
    );
}

#[test]
fn a_message_survives_the_wire_and_an_unknown_code_renders_as_the_code() {
    let m = msg!(TimeAgoMinutes, n = 2);
    let json = serde_json::to_string(&m).unwrap();
    assert_eq!(serde_json::from_str::<Msg>(&json).unwrap(), m);
    assert_eq!(
        serde_json::to_string(&msg!(TimeJustNow)).unwrap(),
        "{\"code\":\"time.just_now\"}"
    );
    let unknown = Msg {
        code: "future.code".into(),
        args: vec![],
    };
    assert_eq!(unknown.render(Lang::De), "future.code");
}

#[test]
fn a_stored_message_whose_code_is_gone_shows_its_stored_text() {
    let stored = "Gespeicherter Satz";
    let gone = Msg {
        code: "core_action.log_step".into(),
        args: vec![],
    };
    assert!(!gone.is_known());
    for lang in Lang::ALL {
        assert_eq!(gone.render_or(lang, stored), stored);
    }
    // Known codes, also plural ones, render from the catalog.
    let known = msg!(TimeAgoMinutes, n = 2);
    assert!(known.is_known());
    assert_eq!(known.render_or(Lang::En, stored), known.render(Lang::En));
}
