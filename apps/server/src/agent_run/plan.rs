//! The plan of what the watcher does next with a run.

use super::*;

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
pub fn plan_instructions(
    ws: &Workspace,
    running: &HashMap<String, Info>,
    blocked: &[String],
) -> Vec<Instruction> {
    let mut a = Vec::new();
    for run in &ws.runs {
        let key = run.key();
        match (running.get(&key), run.state) {
            (None, RunState::Running) if !blocked.contains(&key) => {
                let mode = if matches!(run.kind, RunKind::ReviewPr { .. }) {
                    Mode::Review
                } else {
                    Mode::Implement
                };
                a.push(Instruction::Start(key, mode));
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
        if !ws.runs.iter().any(|r| &r.key() == key) {
            a.push(Instruction::Stop(key.clone()));
        }
    }
    a
}
