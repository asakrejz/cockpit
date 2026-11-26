use super::*;

/// Check and push of a run in one repo – the backend, not the container.
pub(super) async fn publish_single(
    state: &AppState,
    key: &str,
    run: &cockpit_core::AgentRun,
    prepared: Prepared,
    text: String,
    access: &Access,
) -> End {
    let Prepared {
        branch,
        clone,
        mut targets,
        ..
    } = prepared;
    let t = targets.remove(0);
    let (k, e, b, sha, u, tk, rules) = (
        clone.clone(),
        t.export.clone(),
        branch.clone(),
        t.base_sha.clone(),
        t.url.clone(),
        access.github.clone(),
        t.rules.clone(),
    );
    let (pt, base, existing) = (
        t.pattern.clone(),
        run.options.base_branch.clone(),
        run.kind.review().is_some(),
    );
    let published = tokio::task::spawn_blocking(move || {
        let _ = std::fs::remove_dir_all(&e);
        let rule = agent_git::BranchRule {
            branch: &b,
            pattern: &pt,
            base: &base,
            existing,
        };
        agent_git::publish(&k, &e, &sha, &u, Some(&tk), &rules, &rule)
    })
    .await;
    match published {
        Ok(Ok(v)) => {
            // The result per repo is booked for an implementation (a follow-up has none).
            if run.kind == RunKind::Implement {
                let one = cockpit_core::RepoResult {
                    repo: t.repo.clone(),
                    state: cockpit_core::RunRepoState::Pushed,
                    files: v.files.clone(),
                    reason: None,
                };
                let _ = apply(state, key, AgentReport::Repos { repos: vec![one] }).await;
            }
            End::Done(msg!(
                ServerRunDonePushed,
                text = text,
                branch = &branch,
                n = v.files.len()
            ))
        }
        Ok(Err(m)) => End::Error(m),
        Err(_) => End::Error(msg!(ServerRunInternalPush)),
    }
}

/// Each repo is handled on its own: a changed repo is checked (branch pattern, path rules) and
/// pushed, an unchanged one gets nothing, and one that fails its checks is refused without
/// blocking the others. The pull requests are created later, on your click, per repo.
pub(super) async fn publish_repos(
    state: &AppState,
    key: &str,
    run: &cockpit_core::AgentRun,
    targets: Vec<Target>,
    reported: &[agent_run::ResultRepo],
    text: &str,
    access: &Access,
) -> End {
    use cockpit_core::{RepoResult, RunRepoState};
    let mut results: Vec<RepoResult> = Vec::new();
    let mut lines: Vec<Msg> = Vec::new();
    for t in targets {
        // The runner names the branch it committed on; a different one is not pushed.
        let wrong = reported
            .iter()
            .find(|r| r.repo == t.repo)
            .filter(|r| r.branch != t.branch)
            .map(|r| {
                msg!(
                    ServerRunBranchMismatch,
                    got = r.branch.clone(),
                    expected = &t.branch
                )
            });
        let outcome = match wrong {
            Some(m) => Err(m),
            None => {
                let (k, e, b, sha, u, tk, rules, pt, base) = (
                    t.clone.clone(),
                    t.export.clone(),
                    t.branch.clone(),
                    t.base_sha.clone(),
                    t.url.clone(),
                    access.github.clone(),
                    t.rules.clone(),
                    t.pattern.clone(),
                    t.base.clone(),
                );
                match tokio::task::spawn_blocking(move || {
                    let _ = std::fs::remove_dir_all(&e);
                    let rule = agent_git::BranchRule {
                        branch: &b,
                        pattern: &pt,
                        base: &base,
                        existing: false,
                    };
                    agent_git::publish_changes(&k, &e, &sha, &u, Some(&tk), &rules, &rule)
                })
                .await
                {
                    Ok(r) => r,
                    Err(_) => Err(msg!(ServerRunInternalPush)),
                }
            }
        };
        let (state_of, files, reason, line) = match outcome {
            Ok(agent_git::Outcome::Pushed(v)) => (
                RunRepoState::Pushed,
                v.files.clone(),
                None,
                msg!(
                    ServerRunRepoPushed,
                    repo = &t.repo,
                    branch = &t.branch,
                    n = v.files.len()
                ),
            ),
            Ok(agent_git::Outcome::Unchanged) => (
                RunRepoState::Unchanged,
                Vec::new(),
                None,
                msg!(ServerRunRepoUnchanged, repo = &t.repo),
            ),
            Err(m) => (
                RunRepoState::Refused,
                Vec::new(),
                Some(m.clone()),
                msg!(ServerRunRepoRefused, repo = &t.repo, reason = m),
            ),
        };
        results.push(RepoResult {
            repo: t.repo.clone(),
            state: state_of,
            files,
            reason,
        });
        lines.push(line);
    }
    match cockpit_core::judge(&results) {
        cockpit_core::Judgement::Done { .. } => {
            let _ = apply(state, key, AgentReport::Repos { repos: results }).await;
            let _ = run;
            End::Done(msg!(
                ServerRunDoneSummary,
                text = text,
                repos = ArgValue::joined(lines, "\n")
            ))
        }
        cockpit_core::Judgement::NothingChanged => End::Error(msg!(ServerRunNothingChanged)),
        cockpit_core::Judgement::AllRefused { reason } => {
            End::Error(msg!(ServerRunAllReposRefused, reason = reason))
        }
    }
}
