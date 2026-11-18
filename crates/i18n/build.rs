//! Generates the typed keys (`Key` enum) from the English catalog files in `catalog/en/`.
//!
//! One line is `id = text`. `id.one` / `id.other` (and optionally `id.zero`) are the plural forms
//! of the key `id`. Every `catalog/en/<area>.txt` has a partner in `catalog/de/`.

use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn camel(id: &str) -> String {
    id.split(['.', '_', '-'])
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("catalog dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    out.sort();
    out
}

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-changed=catalog");
    let mut bases = BTreeSet::new();
    let mut code = String::new();
    for lang in ["en", "de"] {
        let dir = root.join("catalog").join(lang);
        let paths = files(&dir);
        code.push_str(&format!(
            "pub(crate) const {}_FILES: &[(&str, &str)] = &[\n",
            lang.to_uppercase()
        ));
        for path in &paths {
            let name = path.file_stem().unwrap().to_string_lossy();
            code.push_str(&format!(
                "    ({name:?}, include_str!({:?})),\n",
                path.display().to_string()
            ));
            if lang == "en" {
                let text = fs::read_to_string(path).unwrap();
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let (id, _) = line
                        .split_once('=')
                        .unwrap_or_else(|| panic!("{name}.txt: no `=` in `{line}`"));
                    let id = id.trim();
                    let base = ["one", "other", "zero"]
                        .iter()
                        .find_map(|s| id.strip_suffix(&format!(".{s}")))
                        .unwrap_or(id);
                    bases.insert(base.to_string());
                }
            }
        }
        code.push_str("];\n");
    }
    let mut seen = BTreeSet::new();
    code.push_str("/// Every text of the interface. Generated from `catalog/en/*.txt`.\n");
    code.push_str(
        "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]\npub enum Key {\n",
    );
    for b in &bases {
        let name = camel(b);
        assert!(seen.insert(name.clone()), "keys clash as `{name}`: {b}");
        code.push_str(&format!("    {name},\n"));
    }
    code.push_str("}\n\nimpl Key {\n    pub const ALL: &[Key] = &[\n");
    for b in &bases {
        code.push_str(&format!("        Key::{},\n", camel(b)));
    }
    code.push_str("    ];\n\n    /// The id as written in the catalog files.\n    pub fn id(self) -> &'static str {\n        match self {\n");
    for b in &bases {
        code.push_str(&format!("            Key::{} => {b:?},\n", camel(b)));
    }
    code.push_str("        }\n    }\n}\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("keys.rs"),
        code,
    )
    .unwrap();
}
