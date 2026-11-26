use super::*;

/// Clones `url` to `target`, creates the branch and returns the state from which it starts.
/// The remote is removed afterwards: there is no address and no token in the clone.
pub fn clone(
    url: &str,
    target: &Path,
    token: Option<&Secret>,
    rule: &BranchRule,
) -> Result<String, Msg> {
    let (base_branch, branch) = (rule.base, rule.branch);
    rule.check()?;
    if rule.existing {
        return Err(msg!(ServerRunNewRunNewBranch));
    }
    if !base_branch_ok(base_branch) {
        return Err(msg!(ServerRunBaseBranchInvalid, branch = base_branch));
    }
    branch_is_new(url, token, branch)?;
    let z = target.to_string_lossy().into_owned();
    git(
        &[
            "clone",
            "--quiet",
            "--no-tags",
            "--depth",
            "50",
            "--single-branch",
            "--branch",
            base_branch,
            "--",
            url,
            &z,
        ],
        None,
        url,
        token,
    )?;
    git(
        &["switch", "--quiet", "-c", branch],
        Some(target),
        url,
        None,
    )?;
    let base = git(&["rev-parse", "HEAD"], Some(target), url, None)?
        .trim()
        .to_string();
    git(&["remote", "remove", "origin"], Some(target), url, None)?;
    permissions_open(target);
    Ok(base)
}

/// The contribution guidelines in a clone (`CONTRIBUTING.md` and the two other usual places), as
/// the path inside the repo. Only regular files count. The job file names it so the runner can
/// tell the agent to follow the commit conventions in it.
pub fn guideline_file(clone: &Path) -> Option<&'static str> {
    cockpit_core::branch::GUIDELINE_FILES
        .into_iter()
        .find(|f| std::fs::symlink_metadata(clone.join(f)).is_ok_and(|m| m.file_type().is_file()))
}

/// Clones the **existing** branch `branch` (the branch of a pull request) for a follow-up
/// job and returns the commit it stands on. New commits of the agent build on it; the backend
/// pushes them as a normal push (never a force-push). Like [`clone`], the clone has no remote and
/// no token afterwards.
pub fn clone_branch(
    url: &str,
    target: &Path,
    token: Option<&Secret>,
    rule: &BranchRule,
) -> Result<String, Msg> {
    let branch = rule.branch;
    rule.check()?;
    if !rule.existing {
        return Err(msg!(ServerRunFollowupOnPrBranch));
    }
    clone_existing(url, target, token, branch)
}

/// Clones the branch of any pull request for a **read-only** review run. The agent never pushes
/// from this clone; it is mounted read-only into the container. Otherwise like [`clone_branch`].
pub fn clone_for_review(
    url: &str,
    target: &Path,
    token: Option<&Secret>,
    branch: &str,
) -> Result<String, Msg> {
    if !review_branch_ok(branch) {
        return Err(msg!(ServerRunBranchNotAllowed, branch = branch));
    }
    clone_existing(url, target, token, branch)
}

fn clone_existing(
    url: &str,
    target: &Path,
    token: Option<&Secret>,
    branch: &str,
) -> Result<String, Msg> {
    let z = target.to_string_lossy().into_owned();
    git(
        &[
            "clone",
            "--quiet",
            "--no-tags",
            "--depth",
            "50",
            "--single-branch",
            "--branch",
            branch,
            "--",
            url,
            &z,
        ],
        None,
        url,
        token,
    )?;
    let base = git(&["rev-parse", "HEAD"], Some(target), url, None)?
        .trim()
        .to_string();
    git(&["remote", "remove", "origin"], Some(target), url, None)?;
    permissions_open(target);
    Ok(base)
}
