//! Regeln pro Repository: erlaubte und verbotene Pfade, Pflicht-Tests, Anweisungen an den
//! Agenten und ob der Plan freigegeben werden muss.
//!
//! Alles hier ist rein und ohne Dateisystem: [`glob_match`] vergleicht ein Muster mit einem Pfad,
//! [`RepoRules::violations`] prüft eine Liste von Dateien gegen die Regeln.
//!
//! **Muster:** `*` steht für beliebige Zeichen innerhalb eines Ordnernamens, `?` für genau eines,
//! `**` für beliebig viele Ordner (auch keinen). Ein Muster ohne `/` gilt in jedem Ordner
//! (`*.lock` trifft `Cargo.lock` und `a/b/yarn.lock`), ein Muster mit `/` ab der Wurzel
//! (`styles/*.css`). Ein `/` am Ende meint den ganzen Ordner (`docs/` = `docs/**`).

use serde::{Deserialize, Serialize};

/// Der Name des einzigen Repos, das es heute gibt (Demo). Später kommen mehrere dazu.
pub const DEMO_REPO: &str = "asakrejz/rust-playground";

/// Passt `path` auf `pattern`? Siehe die Regeln im Modul.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = normalize_pattern(pattern);
    if pattern.is_empty() {
        return false;
    }
    let path = path.trim().trim_start_matches("./").trim_start_matches('/');
    let pat: Vec<&str> = pattern.split('/').collect();
    let segs: Vec<&str> = path.split('/').collect();
    match_segments(&pat, &segs)
}

fn normalize_pattern(pattern: &str) -> String {
    let mut p = pattern
        .trim()
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string();
    if p.is_empty() {
        return p;
    }
    if p.ends_with('/') {
        p.push_str("**");
    }
    if !p.contains('/') {
        p.insert_str(0, "**/");
    }
    p
}

fn match_segments(pat: &[&str], segs: &[&str]) -> bool {
    match pat.split_first() {
        None => segs.is_empty(),
        Some((&"**", rest)) => (0..=segs.len()).any(|skip| match_segments(rest, &segs[skip..])),
        Some((first, rest)) => match segs.split_first() {
            Some((seg, tail)) => match_name(first, seg) && match_segments(rest, tail),
            None => false,
        },
    }
}

/// Ein Ordner- oder Dateiname gegen ein Muster mit `*` und `?`.
fn match_name(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0, 0);
    // Letzter Stern und die Stelle im Namen, an der er gerade endet.
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) && p[pi] != '*' {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Warum eine Datei gegen die Regeln verstößt.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViolationKind {
    /// Ein Verbots-Muster trifft die Datei.
    Forbidden,
    /// Es gibt erlaubte Pfade, und die Datei liegt in keinem davon.
    NotAllowed,
}

/// Eine Datei, die der Plan berührt, obwohl die Regeln es nicht zulassen.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PathViolation {
    pub file: String,
    pub kind: ViolationKind,
    /// Das Verbots-Muster, das getroffen hat (bei `NotAllowed` leer).
    #[serde(default)]
    pub pattern: String,
}

impl PathViolation {
    /// Erklärung für die Oberfläche.
    pub fn reason(&self) -> String {
        match self.kind {
            ViolationKind::Forbidden => {
                format!("{} ist verboten (Regel „{}“)", self.file, self.pattern)
            }
            ViolationKind::NotAllowed => {
                format!("{} liegt außerhalb der erlaubten Pfade", self.file)
            }
        }
    }
}

/// Die Regeln eines Repos.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct RepoRules {
    /// Wenn nicht leer, darf der Agent nur diese Pfade ändern (Glob-Muster).
    pub allowed_paths: Vec<String>,
    /// Diese Pfade darf der Agent nie ändern. Verbot schlägt Erlaubnis.
    pub forbidden_paths: Vec<String>,
    /// Befehle, die vor dem Pull Request laufen müssen und grün sein müssen.
    pub required_tests: Vec<String>,
    /// Zusätzliche Anweisungen an den Agenten (Freitext).
    pub instructions: String,
    /// Der Agent legt immer einen Plan vor und wartet auf die Freigabe.
    pub plan_required: bool,
}

fn clean_list(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in list {
        let item = item.trim();
        if !item.is_empty() && !out.iter().any(|o| o == item) {
            out.push(item.to_string());
        }
    }
    out
}

impl RepoRules {
    /// Dieselben Regeln ohne Leerzeichen, leere Zeilen und doppelte Einträge.
    pub fn cleaned(&self) -> Self {
        Self {
            allowed_paths: clean_list(&self.allowed_paths),
            forbidden_paths: clean_list(&self.forbidden_paths),
            required_tests: clean_list(&self.required_tests),
            instructions: self.instructions.trim().to_string(),
            plan_required: self.plan_required,
        }
    }

    /// Gibt es überhaupt Regeln, die den Agenten einschränken oder leiten?
    pub fn is_empty(&self) -> bool {
        *self == RepoRules::default()
    }

    /// Hat die Regel Pfad-Muster?
    pub fn has_path_rules(&self) -> bool {
        !self.allowed_paths.is_empty() || !self.forbidden_paths.is_empty()
    }

    /// Dateien, die gegen die Pfad-Regeln verstoßen. Leer heißt: alles in Ordnung.
    pub fn violations(&self, files: &[String]) -> Vec<PathViolation> {
        files
            .iter()
            .filter_map(|file| {
                if let Some(pattern) = self.forbidden_paths.iter().find(|p| glob_match(p, file)) {
                    return Some(PathViolation {
                        file: file.clone(),
                        kind: ViolationKind::Forbidden,
                        pattern: pattern.clone(),
                    });
                }
                let allowed = self.allowed_paths.is_empty()
                    || self.allowed_paths.iter().any(|p| glob_match(p, file));
                (!allowed).then(|| PathViolation {
                    file: file.clone(),
                    kind: ViolationKind::NotAllowed,
                    pattern: String::new(),
                })
            })
            .collect()
    }

    /// Die geltenden Regeln als Zeilen für Plan und Protokoll. Leer, wenn es keine gibt.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.allowed_paths.is_empty() {
            lines.push(format!("Erlaubt nur: {}", self.allowed_paths.join(", ")));
        }
        if !self.forbidden_paths.is_empty() {
            lines.push(format!("Verboten: {}", self.forbidden_paths.join(", ")));
        }
        if !self.required_tests.is_empty() {
            lines.push(format!(
                "Pflicht-Tests vor dem Pull Request: {}",
                self.required_tests.join(", ")
            ));
        }
        if self.plan_required {
            lines.push("Plan-Freigabe ist Pflicht".to_string());
        }
        if !self.instructions.is_empty() {
            lines.push(format!("Anweisung: {}", self.instructions));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn stars_stay_inside_a_folder_name() {
        assert!(glob_match("styles/*.css", "styles/tokens.css"));
        assert!(!glob_match("styles/*.css", "styles/a/tokens.css"));
        assert!(!glob_match("styles/*.css", "other/styles/tokens.css"));
        assert!(glob_match("src/lib.rs", "src/lib.rs"));
        assert!(!glob_match("src/lib.rs", "src/lib.rs.bak"));
        assert!(glob_match("a*c", "abbbc"));
        assert!(glob_match("a*c", "ac"));
        assert!(!glob_match("a*c", "abbbd"));
        assert!(glob_match("*", "datei"));
        assert!(glob_match("fil?.rs", "file.rs"));
        assert!(!glob_match("fil?.rs", "fil.rs"));
    }

    #[test]
    fn double_star_spans_folders() {
        assert!(glob_match("**/*.rs", "a/b/c.rs"));
        assert!(glob_match("**/*.rs", "c.rs"));
        assert!(glob_match("src/**", "src/a/b.rs"));
        assert!(glob_match("src/**", "src"));
        assert!(!glob_match("src/**", "other/src/a.rs"));
        assert!(glob_match("src/**/mod.rs", "src/mod.rs"));
        assert!(glob_match("src/**/mod.rs", "src/a/b/mod.rs"));
        assert!(!glob_match("src/**/mod.rs", "src/a/b/lib.rs"));
        assert!(glob_match("**", "egal/was"));
    }

    #[test]
    fn patterns_without_a_slash_match_in_every_folder() {
        assert!(glob_match("*.lock", "Cargo.lock"));
        assert!(glob_match("*.lock", "a/b/yarn.lock"));
        assert!(!glob_match("*.lock", "a/lock.txt"));
        assert!(glob_match(".env", "server/.env"));
        assert!(glob_match(".env*", "x/.env.local"));
    }

    #[test]
    fn a_trailing_slash_means_the_whole_folder() {
        assert!(glob_match("docs/", "docs/a/b.md"));
        assert!(glob_match(".github/", ".github/workflows/ci.yml"));
        assert!(!glob_match("docs/", "other/docs.md"));
    }

    #[test]
    fn leading_dot_slash_and_slash_are_ignored() {
        assert!(glob_match("./src/*.rs", "src/a.rs"));
        assert!(glob_match("/src/*.rs", "./src/a.rs"));
        assert!(glob_match("src/*.rs", "/src/a.rs"));
    }

    #[test]
    fn empty_patterns_match_nothing() {
        assert!(!glob_match("", "a"));
        assert!(!glob_match("   ", "a"));
    }

    #[test]
    fn no_rules_allow_everything() {
        let rules = RepoRules::default();
        assert!(rules.is_empty());
        assert!(
            rules
                .violations(&files(&["a.rs", ".github/x.yml"]))
                .is_empty()
        );
        assert!(rules.lines().is_empty());
    }

    #[test]
    fn forbidden_paths_are_reported_with_their_pattern() {
        let rules = RepoRules {
            forbidden_paths: vec![".github/**".into(), "*.lock".into()],
            ..RepoRules::default()
        };
        let v = rules.violations(&files(&["src/a.rs", ".github/ci.yml", "Cargo.lock"]));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].file, ".github/ci.yml");
        assert_eq!(v[0].kind, ViolationKind::Forbidden);
        assert_eq!(v[0].pattern, ".github/**");
        assert_eq!(v[1].pattern, "*.lock");
        assert_eq!(
            v[0].reason(),
            ".github/ci.yml ist verboten (Regel „.github/**“)"
        );
    }

    #[test]
    fn allowed_paths_exclude_everything_else() {
        let rules = RepoRules {
            allowed_paths: vec!["src/**".into(), "tests/**".into()],
            ..RepoRules::default()
        };
        let v = rules.violations(&files(&["src/a.rs", "tests/t.rs", "README.md"]));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].file, "README.md");
        assert_eq!(v[0].kind, ViolationKind::NotAllowed);
        assert_eq!(
            v[0].reason(),
            "README.md liegt außerhalb der erlaubten Pfade"
        );
    }

    #[test]
    fn forbidden_beats_allowed() {
        let rules = RepoRules {
            allowed_paths: vec!["src/**".into()],
            forbidden_paths: vec!["src/secret/**".into()],
            ..RepoRules::default()
        };
        let v = rules.violations(&files(&["src/ok.rs", "src/secret/key.rs"]));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, ViolationKind::Forbidden);
    }

    #[test]
    fn cleaning_trims_and_removes_duplicates_and_blanks() {
        let rules = RepoRules {
            allowed_paths: vec![" src/** ".into(), "".into(), "src/**".into()],
            forbidden_paths: vec!["  ".into()],
            required_tests: vec!["cargo test ".into(), "cargo test".into()],
            instructions: "  Kurz halten.  ".into(),
            plan_required: true,
        }
        .cleaned();
        assert_eq!(rules.allowed_paths, ["src/**"]);
        assert!(rules.forbidden_paths.is_empty());
        assert_eq!(rules.required_tests, ["cargo test"]);
        assert_eq!(rules.instructions, "Kurz halten.");
    }

    #[test]
    fn lines_name_every_active_rule() {
        let rules = RepoRules {
            allowed_paths: vec!["src/**".into()],
            forbidden_paths: vec!["*.lock".into()],
            required_tests: vec!["cargo test".into(), "cargo clippy".into()],
            instructions: "Kleine Commits.".into(),
            plan_required: true,
        };
        assert_eq!(
            rules.lines(),
            [
                "Erlaubt nur: src/**",
                "Verboten: *.lock",
                "Pflicht-Tests vor dem Pull Request: cargo test, cargo clippy",
                "Plan-Freigabe ist Pflicht",
                "Anweisung: Kleine Commits.",
            ]
        );
    }

    #[test]
    fn old_data_without_fields_loads() {
        let rules: RepoRules = serde_json::from_str("{}").unwrap();
        assert!(rules.is_empty());
        let rules: RepoRules = serde_json::from_str(r#"{"plan_required": true}"#).unwrap();
        assert!(rules.plan_required);
    }
}
