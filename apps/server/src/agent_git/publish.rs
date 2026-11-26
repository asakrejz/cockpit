use cockpit_core::RepoRules;
use cockpit_i18n::ArgValue;

use super::*;

/// This many changed files the backend lets through.
const MAX_FILES: usize = 500;

/// What was pushed after the check.
#[derive(Debug, PartialEq, Eq)]
pub struct Pushed {
    pub commit: String,
    pub files: Vec<String>,
}

/// Checks the branch from the clone and pushes it. See the module docs for the order and why
/// nothing is ever worked on in the clone itself. `export` is an empty folder for the bare repo.
/// A branch without new commits or changes is an error here (a run in one repo has nothing to
/// show then); a run in several repos uses [`publish_changes`], where that is a normal result.
pub fn publish(
    clone: &Path,
    export: &Path,
    base_sha: &str,
    url: &str,
    token: Option<&Secret>,
    rules: &RepoRules,
    rule: &BranchRule,
) -> Result<Pushed, Msg> {
    match publish_inner(clone, export, base_sha, url, token, rules, rule)? {
        Published::Pushed(p) => Ok(p),
        Published::NoCommit => Err(msg!(ServerRunNothingCommitted)),
        Published::NoChange => Err(msg!(ServerRunNothingChanged)),
    }
}

/// What a repo of a run in several repos came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The agent left this repo as it was: nothing is pushed.
    Unchanged,
    Pushed(Pushed),
}

/// Like [`publish`], but "nothing changed" is [`Outcome::Unchanged`], not an error. Every check
/// (branch name, history, path rules, new branch) still applies before anything is pushed.
pub fn publish_changes(
    clone: &Path,
    export: &Path,
    base_sha: &str,
    url: &str,
    token: Option<&Secret>,
    rules: &RepoRules,
    rule: &BranchRule,
) -> Result<Outcome, Msg> {
    Ok(
        match publish_inner(clone, export, base_sha, url, token, rules, rule)? {
            Published::Pushed(p) => Outcome::Pushed(p),
            Published::NoCommit | Published::NoChange => Outcome::Unchanged,
        },
    )
}

/// Result of the checks before a push.
enum Published {
    Pushed(Pushed),
    NoCommit,
    NoChange,
}

fn publish_inner(
    clone: &Path,
    export: &Path,
    base_sha: &str,
    url: &str,
    token: Option<&Secret>,
    rules: &RepoRules,
    rule: &BranchRule,
) -> Result<Published, Msg> {
    let branch = rule.branch;
    rule.check()?;
    if base_sha.len() < 7 || !base_sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(msg!(ServerRunStartUnknown));
    }
    let e = export.to_string_lossy().into_owned();
    git(&["init", "--quiet", "--bare", &e], None, "", None)?;
    let k = clone.to_string_lossy().into_owned();
    let spec = format!("+refs/heads/{branch}:refs/heads/{branch}");
    git(
        &["fetch", "--quiet", "--no-tags", "--", &k, &spec],
        Some(export),
        "",
        None,
    )
    .map_err(|m| msg!(ServerRunBranchMissingInClone, branch = branch, error = m))?;
    let ref_ = format!("refs/heads/{branch}");
    let commit = git(&["rev-parse", "--verify", &ref_], Some(export), "", None)?
        .trim()
        .to_string();
    // The branch must build on the starting state (no rewritten history, no foreign one).
    git(
        &["merge-base", "--is-ancestor", base_sha, &commit],
        Some(export),
        "",
        None,
    )
    .map_err(|_| msg!(ServerRunBranchNotBuiltOn))?;
    let count = git(
        &["rev-list", "--count", &format!("{base_sha}..{commit}")],
        Some(export),
        "",
        None,
    )?;
    if count.trim() == "0" {
        return Ok(Published::NoCommit);
    }
    let names = git(
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            base_sha,
            &commit,
        ],
        Some(export),
        "",
        None,
    )?;
    let files: Vec<String> = names
        .split('\0')
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect();
    if files.is_empty() {
        return Ok(Published::NoChange);
    }
    if files.len() > MAX_FILES {
        return Err(msg!(
            ServerRunTooManyFiles,
            n = files.len(),
            max = MAX_FILES
        ));
    }
    let violations = rules.violations(&files);
    if !violations.is_empty() {
        let reasons = ArgValue::joined(violations.iter().map(|v| v.reason_msg()), "; ");
        return Err(msg!(ServerRunRulesViolated, reasons = reasons));
    }
    if !rule.existing {
        // Somebody may have created the branch while the agent worked.
        branch_is_new(url, token, branch)?;
    }
    git(
        &["push", "--quiet", "--", url, &format!("{ref_}:{ref_}")],
        Some(export),
        url,
        token,
    )
    .map_err(|m| msg!(ServerRunPushFailed, branch = branch, error = m))?;
    Ok(Published::Pushed(Pushed { commit, files }))
}
