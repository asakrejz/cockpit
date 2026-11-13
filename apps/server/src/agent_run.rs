//! The real agent, pure part: protocol of the runner, command line for `docker`/`podman`, the
//! job for the container, the prerequisites and the plan of what the watcher does next.
//! Nothing here starts a process or touches the disk – that is done by `agent_ops`.
//!
//! Rules for containers (see "Container" in `docs/ROADMAP.md`): only commands and switches that
//! Docker and Podman understand the same way; secrets only as `-e NAME` (the value is in the
//! environment of the child process, never in the argument list); `--rm`; a label per run for cleaning up;
//! `--add-host cockpit-host:host-gateway` to reach services on the machine.

use std::collections::HashMap;
use std::path::Path;

use cockpit_core::api::{AgentStatus, ContainerStatus, Provider};
use cockpit_core::{AgentReport, AgentRun, RunKind, RunState, Workspace};
use serde::Deserialize;
use serde_json::{Value, json};

/// Name of the agent image (fully named so that Podman does not first search the network for it).
pub const IMAGE: &str = "localhost/cockpit-agent:2";
/// Label by which the backend finds its containers again and cleans up.
pub const LABEL: &str = "cockpit.run";
/// Name under which the container reaches the machine.
pub const HOST_NAME: &str = "cockpit-host";
/// Longer lines of the runner are discarded by the backend.
pub const MAX_LINE: usize = 64 * 1024;
const MAX_TEXT: usize = 2_000;
const MAX_LIST: usize = 200;
/// No single step credibly reports more tokens or time.
const MAX_TOKEN: u64 = 50_000_000;
const MAX_MS: u64 = 3_600_000;

/// What a container does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Only read and present a plan.
    Plan,
    /// Implement the ticket (after approval) and commit.
    Implement,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Mode::Plan => "plan",
            Mode::Implement => "implement",
        }
    }
}

// ---------- Protocol: lines of the runner ----------

/// An event of the runner (a JSON line on stdout). See `agent/README.md`.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Event {
    Plan {
        summary: String,
        #[serde(default)]
        files: Vec<String>,
        #[serde(default)]
        steps: Vec<String>,
    },
    Step {
        title: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        tokens_in: u64,
        #[serde(default)]
        tokens_out: u64,
        #[serde(default)]
        ms: u64,
    },
    Files {
        files: Vec<String>,
    },
    Question {
        text: String,
        #[serde(default)]
        suggestions: Vec<String>,
    },
    HintsArrived,
    Result {
        text: String,
        branch: String,
        commit: String,
    },
    Check {
        reachable: bool,
        key_ok: bool,
        #[serde(default)]
        hint: String,
    },
    Error {
        text: String,
    },
}

/// Why a line was not accepted.
#[derive(Debug, PartialEq, Eq)]
pub enum Discarded {
    TooLarge,
    Unreadable,
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut t: String = text.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

fn list(v: Vec<String>) -> Vec<String> {
    v.into_iter()
        .take(MAX_LIST)
        .map(|s| truncate(&s, 300))
        .collect()
}

/// Reads a line of the runner. Too large, unreadable or unknown lines are discarded;
/// texts, lists and numbers are limited afterwards. The backend does not trust the container.
pub fn line_read(line: &str) -> Result<Event, Discarded> {
    if line.len() > MAX_LINE {
        return Err(Discarded::TooLarge);
    }
    let e: Event = serde_json::from_str(line.trim()).map_err(|_| Discarded::Unreadable)?;
    Ok(match e {
        Event::Plan {
            summary,
            files,
            steps,
        } => Event::Plan {
            summary: truncate(&summary, MAX_TEXT),
            files: list(files),
            steps: list(steps),
        },
        Event::Step {
            title,
            text,
            tokens_in,
            tokens_out,
            ms,
        } => Event::Step {
            title: truncate(&title, 120),
            text: truncate(&text, 300),
            tokens_in: tokens_in.min(MAX_TOKEN),
            tokens_out: tokens_out.min(MAX_TOKEN),
            ms: ms.min(MAX_MS),
        },
        Event::Files { files } => Event::Files { files: list(files) },
        Event::Question { text, suggestions } => Event::Question {
            text: truncate(&text, MAX_TEXT),
            suggestions: list(suggestions).into_iter().take(6).collect(),
        },
        Event::Result {
            text,
            branch,
            commit,
        } => Event::Result {
            text: truncate(&text, 1_500),
            branch: truncate(&branch, 200),
            commit: truncate(&commit, 64),
        },
        Event::Check {
            reachable,
            key_ok,
            hint,
        } => Event::Check {
            reachable,
            key_ok,
            hint: truncate(&hint, 300),
        },
        Event::Error { text } => Event::Error {
            text: truncate(&text, MAX_TEXT),
        },
        same @ Event::HintsArrived => same,
    })
}

/// What an event changes in the run. `None` for events that do not (or not in this mode)
/// belong in the run: result and error are evaluated by the operation itself.
pub fn as_message(e: &Event, mode: Mode) -> Option<AgentReport> {
    match (e, mode) {
        (
            Event::Plan {
                summary,
                files,
                steps,
            },
            Mode::Plan,
        ) => Some(AgentReport::Plan {
            summary: summary.clone(),
            files: files.clone(),
            steps: steps.clone(),
        }),
        (
            Event::Step {
                title,
                text,
                tokens_in,
                tokens_out,
                ms,
            },
            Mode::Plan | Mode::Implement,
        ) => Some(AgentReport::Step {
            title: title.clone(),
            text: text.clone(),
            input_tokens: *tokens_in,
            output_tokens: *tokens_out,
            millis: *ms,
        }),
        (Event::Files { files }, Mode::Implement) => Some(AgentReport::Files {
            files: files.clone(),
        }),
        (Event::Question { text, suggestions }, Mode::Implement) => Some(AgentReport::Question {
            text: text.clone(),
            suggestions: suggestions.clone(),
        }),
        (Event::HintsArrived, Mode::Implement) => Some(AgentReport::HintsDelivered { count: 1 }),
        _ => None,
    }
}

// ---------- Command line ----------

/// Everything `docker run` / `podman run` needs for a run.
pub struct RunCommand<'a> {
    pub name: &'a str,
    /// Value of the label `cockpit.run` (the key of the run).
    pub run: &'a str,
    pub clone: &'a Path,
    /// Read only (plan mode): the container does not change the clone.
    pub clone_only_read: bool,
    pub job: &'a Path,
    /// With open stdin (`-i`) for answers and hints.
    pub stdin_open: bool,
}

/// The arguments after `docker`/`podman`. Never contains a secret: the runner gets address,
/// key and model as `-e NAME` and reads the values from the environment of the child process
/// ([`env_for_kind`]).
pub fn run_args(b: &RunCommand) -> Vec<String> {
    let s = |x: &str| x.to_string();
    let mut a = vec![s("run"), s("--rm")];
    if b.stdin_open {
        a.push(s("-i"));
    }
    a.extend([
        s("--name"),
        s(b.name),
        s("--label"),
        format!("{LABEL}={}", b.run),
        s("--add-host"),
        format!("{HOST_NAME}:host-gateway"),
        s("--cap-drop"),
        s("ALL"),
        s("--security-opt"),
        s("no-new-privileges"),
        s("--pids-limit"),
        s("1024"),
        s("--memory"),
        s("4g"),
        s("-v"),
        format!(
            "{}:/work{}",
            b.clone.display(),
            if b.clone_only_read { ":ro" } else { "" }
        ),
        s("-v"),
        format!("{}:/job:ro", b.job.display()),
    ]);
    for name in KIND_ENV {
        a.extend([s("-e"), s(name)]);
    }
    a.push(s(IMAGE));
    a
}

/// The names that `docker run` passes on via `-e NAME` (only the name, never a value).
pub const KIND_ENV: [&str; 4] = [
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_API_KEY",
];

/// The environment of the `docker run` process: the values that `-e NAME` passes on are there.
/// `Some` sets the value, `None` removes it from the inherited environment so that no key from the
/// person's shell ever travels into the container.
///
/// * With an address (proxy such as LiteLLM, another compatible endpoint): `ANTHROPIC_BASE_URL` and
///   `ANTHROPIC_AUTH_TOKEN` (Bearer).
/// * Without an address (directly at Anthropic): `ANTHROPIC_API_KEY` (`x-api-key`), no base address.
pub fn env_for_kind(
    endpoint_url: Option<&str>,
    key: &str,
    model: &str,
) -> Vec<(&'static str, Option<String>)> {
    let (base, token, api_key) = match endpoint_url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(url) => (Some(for_container(url)), Some(key.to_string()), None),
        None => (None, None, Some(key.to_string())),
    };
    vec![
        ("ANTHROPIC_BASE_URL", base),
        ("ANTHROPIC_AUTH_TOKEN", token),
        ("ANTHROPIC_MODEL", Some(model.to_string())),
        ("ANTHROPIC_API_KEY", api_key),
    ]
}

/// A proxy on the own machine (`localhost`, `127.0.0.1`) is called `cockpit-host` in the container.
pub fn for_container(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let Some((schema, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (host_port, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.ends_with(']') => (h, Some(p)),
        Some((h, p)) if h.ends_with(']') => (h, Some(p)),
        _ => (host_port, None),
    };
    if matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0") {
        let port = port.map(|p| format!(":{p}")).unwrap_or_default();
        return format!("{schema}://{HOST_NAME}{port}{path}");
    }
    url.to_string()
}

/// A name for the container: only allowed characters, unique through the time.
pub fn container_name(run: &str, now_ms: u64) -> String {
    let clean: String = run
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    format!("cockpit-run-{clean}-{now_ms}")
}

/// A folder name for the run (the key contains characters such as `#`).
pub fn run_dir(run: &str) -> String {
    run.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------- Job ----------

/// The job as JSON for the runner (`/job/job.json`). Contains no secrets.
pub fn job_json(ws: &Workspace, run: &AgentRun, mode: Mode, branch: &str) -> Value {
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
        "run_tests": run.options.run_tests,
        "branch": branch,
        "base_branch": run.options.base_branch,
    })
}

/// The job for the reachability check.
pub fn check_job() -> Value {
    json!({ "mode": "check", "ticket": { "key": "-", "title": "-" } })
}

// ---------- Prerequisites ----------

/// What has to be there for a real run.
pub struct Prerequisites<'a> {
    pub container: &'a ContainerStatus,
    pub image: bool,
    /// The tool of the agent; only implemented ones run.
    pub provider: Provider,
    pub model: bool,
    pub key: bool,
    /// The repo in which the agent works; `Err` with the explanation if it is not unambiguous.
    pub repo: Result<&'a str, String>,
    pub github_access: bool,
}

/// One sentence in plain text per missing prerequisite, with the place in the assistant ("Einrichten").
pub fn missing(v: &Prerequisites) -> Vec<String> {
    let mut f = Vec::new();
    match (&v.container.runtime, v.container.engine_running) {
        (None, _) => f.push(
            "Es gibt keine Container-Laufzeit. Öffne „Einrichten“ › „Container“ und installiere Rancher Desktop (dockerd) oder Podman."
                .to_string(),
        ),
        (Some(l), false) => f.push(format!(
            "{l} läuft nicht. Starte es (Rancher Desktop bzw. „podman machine start“) und prüfe es unter „Einrichten“ › „Container“."
        )),
        (Some(_), true) => {
            if !v.image {
                f.push(
                    "Das Agent-Image fehlt. Öffne „Einrichten“ › „Container“ und klicke auf „Image bauen“."
                        .to_string(),
                );
            }
        }
    }
    if let Some(m) = v.provider.not_implemented() {
        f.push(m);
    } else if !v.model || !v.key {
        let mut was = Vec::new();
        if !v.model {
            was.push("der Modellname");
        }
        if !v.key {
            was.push("der Key");
        }
        f.push(format!(
            "Beim Modellzugang fehlt etwas ({}). Trage es unter „Einrichten“ › „Modell“ ein.",
            was.join(", ")
        ));
    }
    if let Err(m) = &v.repo {
        f.push(m.clone());
    }
    if !v.github_access {
        f.push(
            "GitHub ist nicht verbunden (der Agent braucht es zum Klonen und Pushen). Melde dich unter „Einrichten“ › „GitHub“ an."
                .to_string(),
        );
    }
    f
}

/// The repo of the agent: `[agent] repo`, otherwise the only repo from `[github] repos`.
pub fn repo_choose<'a>(agent: Option<&'a str>, github: &'a [String]) -> Result<&'a str, String> {
    if let Some(r) = agent {
        return Ok(r);
    }
    match github {
        [only] => Ok(only.as_str()),
        [] => Err(
            "Es ist kein Repo eingetragen, in dem der Agent arbeiten soll. Trage es unter „Einrichten“ › „GitHub“ ein."
                .to_string(),
        ),
        _ => Err(
            "Es sind mehrere Repos eingetragen. Lege in config.toml unter [agent] mit „repo = \"owner/name\"“ fest, in welchem der Agent arbeitet."
                .to_string(),
        ),
    }
}

/// The status for `/api/health`.
pub fn status(real: bool, v: &Prerequisites) -> AgentStatus {
    if !real {
        return AgentStatus {
            mode: "simulated".into(),
            ready: true,
            image: v.image,
            image_name: IMAGE.into(),
            repo: None,
            missing: Vec::new(),
        };
    }
    let missing = missing(v);
    AgentStatus {
        mode: "real".into(),
        ready: missing.is_empty(),
        image: v.image,
        image_name: IMAGE.into(),
        repo: v.repo.as_ref().ok().map(|r| (*r).to_string()),
        missing,
    }
}

// ---------- Watcher: what is to be done next? ----------

/// What the watcher knows about a running container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub mode: Mode,
    pub paused: bool,
    /// The runner waits for the answer of the user.
    pub question_open: bool,
    /// This many of the waiting hints have already gone to the container.
    pub hints_sent: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Instruction {
    Start(String, Mode),
    /// End the container without changing the run (it was cancelled, halted …).
    Stop(String),
    Pause(String),
    Resume(String),
    Answer(String, String),
    Hints(String, Vec<String>),
}

/// Compares the runs of the workspace with the running containers. Pure: whoever executes them is
/// the operation. `blocked` are runs that have just failed (no immediate restart).
pub fn plane(
    ws: &Workspace,
    running: &HashMap<String, Info>,
    blocked: &[String],
) -> Vec<Instruction> {
    let mut a = Vec::new();
    for run in ws.runs.iter().filter(|r| r.kind == RunKind::Implement) {
        let key = run.key();
        match (running.get(&key), run.state) {
            (None, RunState::Running) if !blocked.contains(&key) => {
                a.push(Instruction::Start(key, Mode::Implement));
            }
            (None, RunState::AwaitingApproval)
                if run.agent_plan.is_none() && !blocked.contains(&key) =>
            {
                a.push(Instruction::Start(key, Mode::Plan));
            }
            (None, _) => {}
            (Some(i), state) => {
                match state {
                    // The plan container is outdated (plan is ready, approved, cancelled …).
                    RunState::AwaitingApproval if i.mode == Mode::Plan => {
                        if run.agent_plan.is_some() {
                            a.push(Instruction::Stop(key));
                        }
                        continue;
                    }
                    RunState::Running | RunState::AwaitingAnswer | RunState::Paused
                        if i.mode == Mode::Plan =>
                    {
                        a.push(Instruction::Stop(key));
                        continue;
                    }
                    RunState::Running | RunState::AwaitingAnswer | RunState::Paused => {}
                    // Cancelled, halted (limit), done, reset …
                    _ => {
                        a.push(Instruction::Stop(key));
                        continue;
                    }
                }
                match (state, i.paused) {
                    (RunState::Paused, false) => a.push(Instruction::Pause(key.clone())),
                    (RunState::Running, true) => a.push(Instruction::Resume(key.clone())),
                    _ => {}
                }
                if state == RunState::Running
                    && i.question_open
                    && let Some(answer) = run
                        .messages
                        .iter()
                        .rev()
                        .find(|m| m.kind == cockpit_core::MessageKind::Answer)
                {
                    a.push(Instruction::Answer(key.clone(), answer.text.clone()));
                }
                if i.hints_sent < run.pending_hints.len() {
                    a.push(Instruction::Hints(
                        key.clone(),
                        run.pending_hints[i.hints_sent..].to_vec(),
                    ));
                }
            }
        }
    }
    // Container without a run (deleted, reset): end it.
    for key in running.keys() {
        if !ws
            .runs
            .iter()
            .any(|r| r.kind == RunKind::Implement && &r.key() == key)
        {
            a.push(Instruction::Stop(key.clone()));
        }
    }
    a
}

#[cfg(test)]
mod tests;
