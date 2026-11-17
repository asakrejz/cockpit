//! The jobs for follow-ups and the check.

use super::*;

/// What a follow-up job gets as input, read from GitHub by the backend when the run starts
/// (limited in amount and size, see `cockpit_integrations::github::followup`). These are texts of
/// third parties: the runner puts them into the prompt as data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FollowupJob {
    /// `review_comments` or `failed_checks`.
    pub kind: &'static str,
    pub review: u32,
    /// The repo (`owner/name`) of the pull request.
    pub repo: String,
    pub threads: Vec<followup::Thread>,
    pub notes: Vec<followup::ReviewNote>,
    pub checks: Vec<followup::FailedCheck>,
}

impl FollowupJob {
    pub fn to_json(&self) -> Value {
        json!({
            "kind": self.kind,
            "pull_request": self.review,
            "repo": self.repo,
            "threads": self.threads,
            "notes": self.notes,
            "checks": self.checks,
        })
    }
}

/// The job as JSON for the runner (`/job/job.json`). Contains no secrets – no token of GitHub, no
/// key of the endpoint (those never go into the job file).
pub fn job_json(
    ws: &Workspace,
    run: &AgentRun,
    mode: Mode,
    branch: &str,
    followup: Option<&FollowupJob>,
) -> Value {
    let item = ws.item_by_key(&run.ticket_key);
    let comments: Vec<String> = item
        .map(|i| {
            i.comments
                .iter()
                .rev()
                .take(10)
                .rev()
                .map(|c| truncate(&format!("{}: {}", c.author.name, c.text), 800))
                .collect()
        })
        .unwrap_or_default();
    let mut hints: Vec<String> = Vec::new();
    if !run.options.hint.trim().is_empty() {
        hints.push(run.options.hint.trim().to_string());
    }
    // Hints that the user gave during the run, and their answers, in case the run restarts.
    let earlier: Vec<String> = run
        .messages
        .iter()
        .rev()
        .take(15)
        .rev()
        .map(|m| truncate(&m.text, 300))
        .collect();
    let rules = &run.rules;
    let instructions: Vec<String> = rules
        .instructions
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    json!({
        "mode": mode.id(),
        "ticket": {
            "key": run.ticket_key,
            "title": run.ticket_title,
            "description": truncate(item.map_or("", |i| i.description.as_str()), 6_000),
            "comments": comments,
        },
        "hints": hints,
        "plan_notes": run.plan_notes,
        "plan": run.agent_plan.as_ref().filter(|_| mode == Mode::Implement).map(|p| json!({
            "summary": p.summary, "files": p.files, "steps": p.steps,
        })),
        "earlier": if run.completed > 0 { earlier } else { Vec::new() },
        "rules": {
            "allowed_paths": rules.allowed_paths,
            "forbidden_paths": rules.forbidden_paths,
            "required_tests": rules.required_tests,
            "instructions": instructions,
        },
        "followup": followup.map(FollowupJob::to_json),
        "run_tests": run.options.run_tests,
        "branch": branch,
        "base_branch": run.options.base_branch,
        "repo": run.repo,
    })
}

/// The job for the reachability check.
pub fn check_job() -> Value {
    json!({ "mode": "check", "ticket": { "key": "-", "title": "-" } })
}
