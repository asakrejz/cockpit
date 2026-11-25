//! Rules per repository: allowed and forbidden paths, required tests, instructions to the
//! agent and whether the plan has to be approved.
//!
//! Everything here is pure and without a file system: [`glob_match`] compares a pattern with a path,
//! [`RepoRules::violations`] checks a list of files against the rules.
//!
//! **Patterns:** `*` stands for any characters within a folder name, `?` for exactly one,
//! `**` for any number of folders (also none). A pattern without `/` applies in every folder
//! (`*.lock` matches `Cargo.lock` and `a/b/yarn.lock`), a pattern with `/` from the root
//! (`styles/*.css`). A trailing `/` means the whole folder (`docs/` = `docs/**`).

use cockpit_i18n::{Msg, msg};
use serde::{Deserialize, Serialize};

/// Does `path` match `pattern`? See the rules in the module.
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

/// A folder or file name against a pattern with `*` and `?`.
fn match_name(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0, 0);
    // Last star and the place in the name where it currently ends.
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

/// Why a file violates the rules.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViolationKind {
    /// A forbidden pattern matches the file.
    Forbidden,
    /// There are allowed paths, and the file is in none of them.
    NotAllowed,
}

/// A file the plan touches although the rules do not allow it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PathViolation {
    pub file: String,
    pub kind: ViolationKind,
    /// The forbidden pattern that matched (empty for `NotAllowed`).
    #[serde(default)]
    pub pattern: String,
}

impl PathViolation {
    /// Explanation for the UI.
    pub fn reason_msg(&self) -> Msg {
        match self.kind {
            ViolationKind::Forbidden => msg!(
                CoreAgentViolationForbidden,
                file = &self.file,
                pattern = &self.pattern
            ),
            ViolationKind::NotAllowed => msg!(CoreAgentViolationNotAllowed, file = &self.file),
        }
    }
}

/// The rules of a repo.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct RepoRules {
    /// If not empty, the agent may change only these paths (glob patterns).
    pub allowed_paths: Vec<String>,
    /// The agent may never change these paths. Prohibition beats permission.
    pub forbidden_paths: Vec<String>,
    /// Commands that must run before the pull request and must be green.
    pub required_tests: Vec<String>,
    /// Additional instructions to the agent (free text).
    pub instructions: String,
    /// The agent always presents a plan and waits for approval.
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
    /// The same rules without spaces, empty lines and duplicate entries.
    pub fn cleaned(&self) -> Self {
        Self {
            allowed_paths: clean_list(&self.allowed_paths),
            forbidden_paths: clean_list(&self.forbidden_paths),
            required_tests: clean_list(&self.required_tests),
            instructions: self.instructions.trim().to_string(),
            plan_required: self.plan_required,
        }
    }

    /// Are there any rules at all that restrict or guide the agent?
    pub fn is_empty(&self) -> bool {
        *self == RepoRules::default()
    }

    /// Does the rule have path patterns?
    pub fn has_path_rules(&self) -> bool {
        !self.allowed_paths.is_empty() || !self.forbidden_paths.is_empty()
    }

    /// Files that violate the path rules. Empty means: all fine.
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

    /// The applicable rules as lines for plan and log. Empty if there are none.
    pub fn lines_msg(&self) -> Vec<Msg> {
        let mut lines = Vec::new();
        if !self.allowed_paths.is_empty() {
            lines.push(msg!(
                CoreAgentRuleAllowed,
                paths = self.allowed_paths.join(", ")
            ));
        }
        if !self.forbidden_paths.is_empty() {
            lines.push(msg!(
                CoreAgentRuleForbidden,
                paths = self.forbidden_paths.join(", ")
            ));
        }
        if !self.required_tests.is_empty() {
            lines.push(msg!(
                CoreAgentRuleRequiredTests,
                tests = self.required_tests.join(", ")
            ));
        }
        if self.plan_required {
            lines.push(msg!(CoreAgentRulePlanRequired));
        }
        if !self.instructions.is_empty() {
            lines.push(msg!(CoreAgentRuleInstruction, text = &self.instructions));
        }
        lines
    }
}

#[cfg(test)]
mod tests;
