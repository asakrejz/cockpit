//! May a branch of the agent be deleted? Pure rules; the backend adds the facts it reads from
//! GitHub. Deleting is never automatic: it only happens on the user's click, and only when every
//! rule here says yes.

use super::{check, structure};
use crate::agent::Closure;

/// What is known about the branch and its pull request.
#[derive(Clone, Copy, Debug)]
pub struct DeleteFacts<'a> {
    /// The branch pattern of the repo.
    pub pattern: &'a str,
    /// The base branch the repo is configured with.
    pub base_branch: &'a str,
    /// The default branch of the repo on GitHub.
    pub default_branch: &'a str,
    /// The branch the user wants to delete.
    pub branch: &'a str,
    /// The branch GitHub reports as head of the pull request.
    pub pull_head: &'a str,
    /// The branch the run pushed (what the run's options render).
    pub run_branch: &'a str,
    /// GitHub marks the branch as protected.
    pub protected: bool,
    /// How the pull request ended; `None` while it is still open.
    pub ended: Option<Closure>,
}

/// `Ok` if the branch may be deleted, otherwise the reason in plain German.
///
/// ```
/// use cockpit_core::branch::{DeleteFacts, deletable};
/// use cockpit_core::Closure;
/// let facts = DeleteFacts {
///     pattern: "claude/{ticket}-{slug}",
///     base_branch: "main",
///     default_branch: "main",
///     branch: "claude/BAU-1-x",
///     pull_head: "claude/BAU-1-x",
///     run_branch: "claude/BAU-1-x",
///     protected: false,
///     ended: Some(Closure::Merged),
/// };
/// assert!(deletable(&facts).is_ok());
/// assert!(deletable(&DeleteFacts { ended: None, ..facts }).is_err());
/// ```
pub fn deletable(f: &DeleteFacts) -> Result<(), String> {
    structure(f.branch)?;
    if f.ended.is_none() {
        return Err("Der Pull Request ist noch offen. Den Branch kannst du erst löschen, wenn er gemergt oder geschlossen ist.".into());
    }
    if f.branch.eq_ignore_ascii_case(f.default_branch) {
        return Err(format!(
            "Der Branch „{}“ ist der Standard-Branch des Repos und bleibt.",
            f.branch
        ));
    }
    // Base branch, main branches and the pattern of the repo.
    check(f.pattern, f.base_branch, f.branch)?;
    if f.protected {
        return Err(format!(
            "Der Branch „{}“ ist auf GitHub geschützt und bleibt.",
            f.branch
        ));
    }
    if f.branch != f.pull_head || f.branch != f.run_branch {
        return Err(format!(
            "Der Branch „{}“ gehört nicht zu diesem Lauf und seinem Pull Request.",
            f.branch
        ));
    }
    Ok(())
}
