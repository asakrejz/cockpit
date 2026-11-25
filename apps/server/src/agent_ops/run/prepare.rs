use super::*;

/// One repo of a run in several repos, with everything needed to clone, check and push it.
pub(super) struct Target {
    pub repo: String,
    pub branch: String,
    pub pattern: String,
    pub base: String,
    pub url: String,
    pub clone: PathBuf,
    pub base_file: PathBuf,
    pub export: PathBuf,
    pub rules: cockpit_core::RepoRules,
    pub base_sha: String,
}

/// What `prepare` leaves for the next steps: the branch, the clones and the folder of the job file.
pub(super) struct Prepared {
    pub branch: String,
    /// A run in several repos: every repo has its own clone below `clone`.
    pub multi: bool,
    /// The folder of the clone (of all clones, with `multi`) that the container gets as `/work`.
    pub clone: PathBuf,
    pub targets: Vec<Target>,
    pub job_dir: PathBuf,
}

pub(super) fn job_write(dir: &Path, content: &serde_json::Value) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join("job.json");
    std::fs::write(&file, content.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

/// Everything before the container: the follow-up inputs, one clone per repo and the job file.
pub(super) async fn prepare(
    state: &AppState,
    key: &str,
    mode: Mode,
    ws: &Workspace,
    run: &cockpit_core::AgentRun,
    access: &Access,
    secret_refs: &[&str],
) -> Result<Prepared, Msg> {
    let repo = run.repo.clone();

    // Follow-up job: the inputs come from GitHub (read only, with the token of the person) and go
    // into the job file as text. Whatever looks like one of our keys is removed first.
    let followup = match state.followup_job(ws, &run.kind).await {
        Ok(Some(job)) => {
            let text = agent_git::redact(&job.to_json().to_string(), secret_refs);
            serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .map(|v| (job, v))
        }
        Ok(None) => None,
        Err(m) => return Err(m),
    };

    let branch = run.branch();
    let base_dir = state.agent_dir().join("runs").join(agent_run::run_dir(key));
    let mode_dir = base_dir.join(match mode {
        Mode::Plan => "plan",
        Mode::Review => "review",
        Mode::Implement => "run",
    });
    // A run in several repos (only "implement ticket" and its plan): every repo gets its own
    // clone in `<mode>/<owner>__<name>`, and the container sees `/work/<owner>__<name>`. A run in
    // one repo keeps the clone directly in `/work`.
    let multi = !run.is_followup() && mode != Mode::Review && run.repos.len() > 1;
    let mut targets: Vec<Target> = Vec::new();
    if multi {
        for entry in &run.repos {
            let name = match state.run_repo_of(run, &entry.repo) {
                Ok(r) => r,
                Err(m) => return Err(m),
            };
            let dir = cockpit_core::repo_dir(&name);
            targets.push(Target {
                branch: run.branch_in(&name),
                // The pattern comes from the configuration of the repo, never from the run's own options.
                pattern: state.agent_repos().branch_pattern(&name),
                base: if entry.base_branch.is_empty() {
                    run.options.base_branch.clone()
                } else {
                    entry.base_branch.clone()
                },
                url: agent_git::clone_url(&access.git_base, &name),
                clone: mode_dir.join(&dir),
                base_file: base_dir.join(format!("base-{dir}")),
                export: base_dir.join(format!("export-{dir}.git")),
                rules: entry.rules.clone(),
                base_sha: String::new(),
                repo: name,
            });
        }
    } else {
        targets.push(Target {
            branch: branch.clone(),
            // The pattern comes from the configuration of the repo, never from the run's own options.
            pattern: state.agent_repos().branch_pattern(&repo),
            base: run.options.base_branch.clone(),
            url: agent_git::clone_url(&access.git_base, &repo),
            clone: mode_dir.clone(),
            base_file: base_dir.join("base"),
            export: base_dir.join("export.git"),
            rules: run.rules.clone(),
            base_sha: String::new(),
            repo: repo.clone(),
        });
    }
    let clone = mode_dir.clone();

    // Clone: fresh, unless a halted run is resumed.
    let present = mode == Mode::Implement
        && targets
            .iter()
            .all(|t| t.clone.join(".git").is_dir() && t.base_file.is_file());
    for t in &mut targets {
        if present {
            t.base_sha = std::fs::read_to_string(&t.base_file)
                .unwrap_or_default()
                .trim()
                .to_string();
            continue;
        }
        let (k, u, tk, b, br, pt) = (
            t.clone.clone(),
            t.url.clone(),
            access.github.clone(),
            t.base.clone(),
            t.branch.clone(),
            t.pattern.clone(),
        );
        let on_existing_branch = run.kind.review().is_some();
        let read_only_review = mode == Mode::Review;
        let bd = t.base_file.clone();
        let cloned = tokio::task::spawn_blocking(move || {
            let _ = std::fs::remove_dir_all(&k);
            if let Some(p) = k.parent() {
                std::fs::create_dir_all(p)
                    .map_err(|e| msg!(ServerRunDirNotCreatable, error = e.to_string()))?;
            }
            // A follow-up job continues on the branch of the pull request; a new run branches off.
            let rule = agent_git::BranchRule {
                branch: &br,
                pattern: &pt,
                base: &b,
                existing: on_existing_branch,
            };
            let sha = if read_only_review {
                // Any branch of the pull request: the agent only reads this clone.
                agent_git::clone_for_review(&u, &k, Some(&tk), &br)?
            } else if on_existing_branch {
                agent_git::clone_branch(&u, &k, Some(&tk), &rule)?
            } else {
                agent_git::clone(&u, &k, Some(&tk), &rule)?
            };
            std::fs::write(&bd, &sha)
                .map_err(|e| msg!(ServerRunStateNotSaved, error = e.to_string()))?;
            Ok::<_, Msg>(sha)
        })
        .await;
        match cloned {
            Ok(Ok(sha)) => t.base_sha = sha,
            Ok(Err(m)) => {
                // The git errors are redacted where they are made.
                return Err(msg!(ServerRunCloneFailed, repo = &t.repo, reason = m));
            }
            Err(_) => return Err(msg!(ServerRunInternalClone)),
        }
    }

    let job_dir = base_dir.join(format!("job-{}", mode.id()));
    let mut job = agent_run::job_json(ws, run, mode, &branch, followup.as_ref().map(|f| &f.0));
    if let Some((_, redacted)) = &followup {
        job["followup"] = redacted.clone();
    }
    if multi {
        let guides: HashMap<String, &'static str> = targets
            .iter()
            .filter_map(|t| agent_git::guideline_file(&t.clone).map(|f| (t.repo.clone(), f)))
            .collect();
        job["repos"] = agent_run::repos_json(ws, run, &guides);
    } else if let Some(file) = targets
        .first()
        .and_then(|t| agent_git::guideline_file(&t.clone))
    {
        job["contributing"] = file.into();
    }
    // The diff and the description come from third parties: whatever looks like one of our keys goes.
    if let Some(review) = job.get("review").filter(|v| !v.is_null()) {
        let text = agent_git::redact(&review.to_string(), secret_refs);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            job["review"] = v;
        }
    }
    if let Err(e) = job_write(&job_dir, &job) {
        return Err(msg!(ServerRunJobNotWritable, error = e.to_string()));
    }
    Ok(Prepared {
        branch,
        multi,
        clone,
        targets,
        job_dir,
    })
}
