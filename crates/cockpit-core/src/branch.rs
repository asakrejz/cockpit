//! Branch names of the agent: the pattern per repo, rendering, and the check before a push.
//!
//! The pattern is set per repo (`[[agent.repos]] branch_pattern`). Placeholders:
//!
//! * `{ticket}`: the Jira key (`BAU-142`). Every pattern needs it, so two tickets never share a branch.
//! * `{slug}`: the ticket title as a short name (`toggle-fokus-ring-fehlt`).
//! * `{type}`: `feature`, `fix` or `chore`, derived from the Jira issue type ([`BranchType`]).
//!
//! The backend renders the branch with [`render`] and checks it with [`check`] before it clones and
//! again before it pushes: a branch that does not match the pattern, or that is a protected
//! name, never leaves the machine. Everything here is pure.

mod delete;
pub use delete::{DeleteFacts, deletable};
mod suggest;
pub use suggest::{GUIDELINE_FILES, Suggestion, suggest};

/// Pattern of a repo that sets none.
pub const DEFAULT_PATTERN: &str = "claude/{ticket}-{slug}";

/// Names the agent never pushes to, whatever the pattern says (compared in lowercase).
const PROTECTED: &[&str] = &[
    "main",
    "master",
    "head",
    "develop",
    "development",
    "trunk",
    "production",
    "prod",
    "staging",
];

/// Longest branch name the backend accepts.
const MAX_BRANCH: usize = 120;

/// Longest pattern.
const MAX_PATTERN: usize = 100;

/// What `{type}` stands for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BranchType {
    #[default]
    Feature,
    Fix,
    Chore,
}

impl BranchType {
    pub fn as_str(self) -> &'static str {
        match self {
            BranchType::Feature => "feature",
            BranchType::Fix => "fix",
            BranchType::Chore => "chore",
        }
    }

    /// Reads the three words back (`feature`, `fix`, `chore`); anything else is `Feature`.
    pub fn from_word(word: &str) -> BranchType {
        match word.trim() {
            "fix" => BranchType::Fix,
            "chore" => BranchType::Chore,
            _ => BranchType::Feature,
        }
    }

    /// The type for a Jira issue type (any language, any case); the labels of the ticket help
    /// when Jira gives no type. Unknown or missing types are a feature.
    ///
    /// ```
    /// use cockpit_core::branch::BranchType;
    /// assert_eq!(BranchType::of(Some("Bug"), &[]), BranchType::Fix);
    /// assert_eq!(BranchType::of(Some("Story"), &[]), BranchType::Feature);
    /// assert_eq!(BranchType::of(Some("Sub-task"), &[]), BranchType::Chore);
    /// assert_eq!(BranchType::of(None, &["bug".to_string()]), BranchType::Fix);
    /// ```
    pub fn of(issue_type: Option<&str>, labels: &[String]) -> BranchType {
        let by_word = |w: &str| {
            let w = w.trim().to_lowercase();
            let has = |list: &[&str]| list.iter().any(|k| w.contains(k));
            if has(&["bug", "defect", "fehler", "incident", "hotfix", "störung"]) {
                Some(BranchType::Fix)
            } else if has(&[
                "task",
                "aufgabe",
                "chore",
                "maintenance",
                "wartung",
                "tech",
                "spike",
            ]) {
                Some(BranchType::Chore)
            } else if has(&[
                "story",
                "epic",
                "feature",
                "improvement",
                "funktion",
                "verbesser",
            ]) {
                Some(BranchType::Feature)
            } else {
                None
            }
        };
        issue_type
            .filter(|t| !t.trim().is_empty())
            .and_then(by_word)
            .or_else(|| labels.iter().find_map(|l| by_word(l)))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part<'a> {
    Text(&'a str),
    Ticket,
    Slug,
    Type,
}

/// Splits a pattern into text and placeholders; an unknown or unclosed placeholder is an error.
fn parts(pattern: &str) -> Result<Vec<Part<'_>>, String> {
    let mut out = Vec::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        let Some(open) = rest.find(['{', '}']) else {
            out.push(Part::Text(rest));
            break;
        };
        if open > 0 {
            out.push(Part::Text(&rest[..open]));
        }
        let tail = &rest[open..];
        let Some(close) = tail.strip_prefix('{').and_then(|t| t.find('}')) else {
            return Err("Eine geschweifte Klammer im Muster ist nicht geschlossen.".into());
        };
        let name = &tail[1..=close];
        out.push(match name {
            "ticket" => Part::Ticket,
            "slug" => Part::Slug,
            "type" => Part::Type,
            other => {
                return Err(format!(
                    "„{{{other}}}“ kennt Cockpit nicht. Erlaubt sind {{ticket}}, {{slug}} und {{type}}."
                ));
            }
        });
        rest = &tail[close + 2..];
    }
    Ok(out)
}

/// Is the pattern usable? The message is plain German and goes to the user.
///
/// A pattern needs `{ticket}`; it may only contain letters, digits and `/ - _ . { }`; it never
/// begins or ends with a separator, and two placeholders are never glued together.
///
/// ```
/// use cockpit_core::branch::validate;
/// assert!(validate("feature/{ticket}-{slug}").is_ok());
/// assert!(validate("main").is_err());
/// assert!(validate("").is_err());
/// assert!(validate("fix/{slug}").is_err());
/// ```
pub fn validate(pattern: &str) -> Result<(), String> {
    let p = pattern.trim();
    if p.is_empty() {
        return Err("Das Muster für Branch-Namen darf nicht leer sein.".into());
    }
    if p.len() > MAX_PATTERN {
        return Err(format!(
            "Das Muster für Branch-Namen ist zu lang (höchstens {MAX_PATTERN} Zeichen)."
        ));
    }
    if !p
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '{' | '}'))
    {
        return Err(
            "Das Muster darf nur Buchstaben, Ziffern und die Zeichen / - _ . sowie Platzhalter enthalten."
                .into(),
        );
    }
    let list = parts(p)?;
    if !list.iter().any(|x| !matches!(x, Part::Text(_))) {
        return Err(
            "Das Muster braucht mindestens einen Platzhalter, sonst zeigt es auf einen festen Branch."
                .into(),
        );
    }
    if !list.contains(&Part::Ticket) {
        return Err(
            "Das Muster braucht {ticket}, sonst bekämen zwei Tickets denselben Branch.".into(),
        );
    }
    if list
        .windows(2)
        .any(|w| !matches!(w[0], Part::Text(_)) && !matches!(w[1], Part::Text(_)))
    {
        return Err(
            "Zwischen zwei Platzhaltern muss ein Zeichen stehen, z. B. {ticket}-{slug}.".into(),
        );
    }
    if p.starts_with(['-', '/', '.'])
        || p.contains("..")
        || p.contains("//")
        || p.ends_with(['/', '.'])
        || p.ends_with(".lock")
    {
        return Err("Das Muster darf nicht mit / - . beginnen oder mit / . enden und kein .. oder // enthalten.".into());
    }
    // The shape must also hold for what the pattern renders.
    let sample = render(p, "ABC-1", "Beispiel Titel", BranchType::Feature);
    structure(&sample)
        .map_err(|_| "Aus diesem Muster entstünde kein gültiger Branch-Name.".to_string())
}

/// The branch for a ticket. The pattern has to be valid (see [`validate`]); an invalid one is
/// rendered as the default pattern.
///
/// ```
/// use cockpit_core::branch::{BranchType, render};
/// assert_eq!(
///     render("feature/{ticket}-{slug}", "BAU-142", "Fokus-Ring fehlt", BranchType::Fix),
///     "feature/BAU-142-fokus-ring-fehlt"
/// );
/// assert_eq!(render("{type}/{ticket}", "BAU-1", "x", BranchType::Fix), "fix/BAU-1");
/// ```
pub fn render(pattern: &str, ticket: &str, title: &str, kind: BranchType) -> String {
    let list = parts(pattern.trim())
        .ok()
        .filter(|_| validate_shape(pattern))
        .unwrap_or_else(|| parts(DEFAULT_PATTERN).unwrap_or_default());
    let mut out = String::new();
    for p in list {
        match p {
            Part::Text(t) => out.push_str(t),
            Part::Ticket => out.push_str(ticket),
            Part::Slug => out.push_str(&crate::text::slug(title, 24)),
            Part::Type => out.push_str(kind.as_str()),
        }
    }
    // An empty slug (a title without letters) must not leave a dangling separator.
    out.trim_end_matches(['-', '_', '.', '/']).to_string()
}

/// Cheap check used by [`render`] so it never recurses into [`validate`].
fn validate_shape(pattern: &str) -> bool {
    let p = pattern.trim();
    !p.is_empty() && p.len() <= MAX_PATTERN && parts(p).is_ok_and(|l| l.contains(&Part::Ticket))
}

/// Does `branch` come from `pattern`? Placeholders match what they render: a Jira key, a slug,
/// one of the three types.
///
/// ```
/// use cockpit_core::branch::matches;
/// assert!(matches("feature/{ticket}-{slug}", "feature/BAU-142-fokus-ring"));
/// assert!(!matches("feature/{ticket}-{slug}", "claude/BAU-142-fokus-ring"));
/// assert!(!matches("feature/{ticket}-{slug}", "feature/main"));
/// ```
pub fn matches(pattern: &str, branch: &str) -> bool {
    let Ok(list) = parts(pattern.trim()) else {
        return false;
    };
    if !validate_shape(pattern) {
        return false;
    }
    fits(&list, branch)
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_ticket(s: &str) -> bool {
    let Some((project, number)) = s.rsplit_once('-') else {
        return false;
    };
    !project.is_empty()
        && project
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// Matches the parts against `rest`, trying every length for a placeholder (backtracking).
fn fits(list: &[Part<'_>], rest: &str) -> bool {
    let Some((first, tail)) = list.split_first() else {
        return rest.is_empty();
    };
    match first {
        Part::Text(t) => {
            // `render` drops the separator in front of an empty slug at the end.
            let slug_dropped =
                tail == [Part::Slug] && rest == t.trim_end_matches(['-', '_', '.', '/']);
            slug_dropped || rest.strip_prefix(t).is_some_and(|r| fits(tail, r))
        }
        Part::Type => ["feature", "fix", "chore"]
            .iter()
            .any(|w| rest.strip_prefix(w).is_some_and(|r| fits(tail, r))),
        Part::Ticket | Part::Slug => {
            let ok = |s: &str| {
                if *first == Part::Ticket {
                    is_ticket(s)
                } else {
                    is_slug(s)
                }
            };
            (1..=rest.len())
                .filter(|&n| rest.is_char_boundary(n))
                .any(|n| ok(&rest[..n]) && fits(tail, &rest[n..]))
        }
    }
}

/// Branch names Git accepts and the backend lets through: inconspicuous characters only.
fn structure(branch: &str) -> Result<(), String> {
    let bad = branch.is_empty()
        || branch.len() > MAX_BRANCH
        || branch.starts_with(['-', '/', '.'])
        || branch.contains("..")
        || branch.contains("//")
        || branch.ends_with('/')
        || branch.ends_with(".lock")
        || branch.ends_with('.')
        || !branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'));
    if bad {
        Err(format!(
            "Der Branch „{branch}“ ist kein gültiger Branch-Name."
        ))
    } else {
        Ok(())
    }
}

/// A name the agent must never push to: the main branches, the base branch of the repo.
pub fn is_protected(branch: &str, base_branch: &str) -> bool {
    let lower = branch.to_lowercase();
    lower == base_branch.to_lowercase()
        || PROTECTED.contains(&lower.as_str())
        || lower.starts_with("refs/")
}

/// The check before the agent clones, and before the backend pushes: a valid name, not a
/// protected one, and one that the pattern of the repo produces. The message is plain German.
///
/// ```
/// use cockpit_core::branch::check;
/// let pattern = "feature/{ticket}-{slug}";
/// assert!(check(pattern, "main", "feature/BAU-1-x").is_ok());
/// assert!(check(pattern, "main", "main").is_err());
/// assert!(check(pattern, "main", "claude/BAU-1-x").is_err());
/// ```
pub fn check(pattern: &str, base_branch: &str, branch: &str) -> Result<(), String> {
    structure(branch)?;
    if is_protected(branch, base_branch) {
        return Err(format!(
            "Der Branch „{branch}“ ist geschützt – dorthin pusht der Agent nie."
        ));
    }
    if !matches(pattern, branch) {
        return Err(format!(
            "Der Branch „{branch}“ passt nicht zum Muster „{}“ dieses Repos.",
            pattern.trim()
        ));
    }
    Ok(())
}

/// Like [`check`], for a branch that already exists (a follow-up on a pull request): besides the
/// pattern of the repo, anything under `claude/` counts, because pull requests of the agent from
/// before the pattern was changed keep their name (and the agent only ever added to such a
/// branch before this setting existed).
pub fn check_existing(pattern: &str, base_branch: &str, branch: &str) -> Result<(), String> {
    check(pattern, base_branch, branch).or_else(|first| {
        let legacy = structure(branch).is_ok()
            && !is_protected(branch, base_branch)
            && branch
                .strip_prefix("claude/")
                .is_some_and(|r| !r.is_empty());
        if legacy { Ok(()) } else { Err(first) }
    })
}

/// Does the branch look like one of the agent? Used by the interface to offer follow-up jobs.
pub fn is_agent_branch(pattern: &str, base_branch: &str, branch: &str) -> bool {
    check_existing(pattern, base_branch, branch).is_ok()
}

#[cfg(test)]
mod tests;
