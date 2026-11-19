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
        "review": if mode == Mode::Review { review_input(ws, run) } else { None },
        "run_tests": run.options.run_tests,
        "branch": branch,
        "base_branch": run.options.base_branch,
        "repo": run.repo,
        // The agent writes plan, summary and comments in the language of the interface; commit
        // message and pull request text follow the language of the repo.
        "language": ws.language.unwrap_or_default().code(),
        "pr_language": run.options.pr_language.code(),
    })
}

/// Largest diff text (characters) that goes into a review job; further files are left out.
const REVIEW_BUDGET: usize = 80_000;
/// Longest single line of the diff.
const REVIEW_LINE: usize = 300;
/// Most lines of one file.
const REVIEW_FILE_LINES: usize = 1_500;

/// What a review run gets to look at: title, description and the diff of the pull request, taken
/// from the state of the workspace (nothing is fetched from GitHub). These are texts of third
/// parties – the runner puts them into the prompt as data. Limited in size.
pub fn review_input(ws: &Workspace, run: &AgentRun) -> Option<Value> {
    let key = run.kind.review()?;
    let r = ws.review(&key)?;
    let mut budget = REVIEW_BUDGET;
    let mut files = Vec::new();
    let mut omitted = 0usize;
    for f in &r.files {
        if budget == 0 {
            omitted += 1;
            continue;
        }
        let mut lines = Vec::new();
        for l in f.lines.iter().take(REVIEW_FILE_LINES) {
            let text = truncate(&l.text, REVIEW_LINE);
            let cost = text.chars().count() + 12;
            if cost > budget {
                break;
            }
            budget -= cost;
            let kind = match l.kind {
                cockpit_core::LineKind::Add => "add",
                cockpit_core::LineKind::Del => "del",
                cockpit_core::LineKind::Context => "ctx",
            };
            lines.push(json!({ "no": l.no, "kind": kind, "text": text }));
        }
        files.push(json!({ "name": truncate(&f.name, 300), "lines": lines }));
    }
    Some(json!({
        "pull_request": r.number,
        "title": truncate(&r.title, 300),
        "description": truncate(&r.description, 6_000),
        "branch": truncate(&r.branch, 200),
        "base": truncate(&r.base, 200),
        "files": files,
        "omitted_files": omitted,
    }))
}

/// The job for the reachability check.
pub fn check_job() -> Value {
    json!({ "mode": "check", "ticket": { "key": "-", "title": "-" } })
}

/// The repos of a run in several repos for the runner (`repos` in the job file). Each repo is
/// cloned to `/work/<dir>` and has its own branch, base branch and rules; the runner tells the
/// agent about all of them. `guides` maps a repo to the path of its contribution guidelines.
pub fn repos_json(run: &AgentRun, guides: &HashMap<String, &'static str>) -> Value {
    let list: Vec<Value> = run
        .repos
        .iter()
        .map(|r| {
            let instructions: Vec<String> = r
                .rules
                .instructions
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            json!({
                "repo": r.repo,
                "dir": r.dir(),
                "branch": run.branch_in(&r.repo),
                "base_branch": r.base_branch,
                "pr_language": r.pr_language.code(),
                "contributing": guides.get(&r.repo).copied().unwrap_or(""),
                "rules": {
                    "allowed_paths": r.rules.allowed_paths,
                    "forbidden_paths": r.rules.forbidden_paths,
                    "required_tests": r.rules.required_tests,
                    "instructions": instructions,
                },
            })
        })
        .collect();
    Value::Array(list)
}
