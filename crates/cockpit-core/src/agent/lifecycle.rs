use super::*;
use cockpit_i18n::{Msg, msg};

impl AgentRun {
    pub fn start(&mut self, mut options: AgentOptions) {
        // The rules of the repo come first: required tests cannot be switched off, and whoever
        // violates the path rules or whose repo demands it always presents a plan.
        if !self.rules.required_tests.is_empty() {
            options.run_tests = true;
        }
        if self.rules.plan_required || !self.violations().is_empty() {
            options.plan_first = true;
        }
        self.planned = options.plan_first;
        self.state = if self.planned {
            RunState::AwaitingApproval
        } else {
            RunState::Running
        };
        self.options = options;
        self.completed = 0;
        self.pr_number = self.kind.review().map(|k| k.number);
        self.plan_notes.clear();
        self.agent_plan = None;
        self.ended = None;
        self.closure = None;
        self.summary.clear();
        self.clear_usage();
        self.clear_conversation();
        self.say_msg(
            Speaker::Agent,
            MessageKind::Info,
            msg!(CoreAgentMAccepted, label = self.label_msg()),
        );
        let hint = self.options.hint.trim().to_string();
        if !hint.is_empty() {
            self.say(Speaker::User, MessageKind::Hint, hint);
        }
        if self.planned {
            self.say_msg(
                Speaker::Agent,
                MessageKind::Info,
                msg!(CoreAgentMPlanPresented),
            );
        }
    }

    pub(super) fn clear_usage(&mut self) {
        self.step_usage.clear();
        self.extra_budget_cents = 0;
        self.extra_minutes = 0;
    }

    pub(super) fn clear_conversation(&mut self) {
        self.messages.clear();
        self.question = None;
        self.asked = false;
        self.pending_hints.clear();
    }

    pub(super) fn say(&mut self, from: Speaker, kind: MessageKind, text: String) {
        let step = self.completed;
        self.messages.push(RunMessage {
            from,
            kind,
            step,
            text,
            msg: None,
        });
    }

    /// Like [`AgentRun::say`], for a sentence of the system as code and parameters.
    pub(super) fn say_msg(&mut self, from: Speaker, kind: MessageKind, msg: Msg) {
        let step = self.completed;
        self.messages.push(RunMessage {
            from,
            kind,
            step,
            text: msg.de(),
            msg: Some(msg),
        });
    }

    /// Cancels or resets: the run is ready to start again afterwards.
    pub fn reset(&mut self) {
        self.state = RunState::Idle;
        self.completed = 0;
        self.pr_number = self.kind.review().map(|k| k.number);
        self.plan_notes.clear();
        self.planned = false;
        self.agent_plan = None;
        self.summary.clear();
        self.clear_usage();
        self.clear_conversation();
    }

    /// Approves the plan: from now on the agent works. `false` if there is nothing to approve.
    pub fn approve(&mut self) -> bool {
        // A plan that touches forbidden paths cannot be approved.
        if !self.is_awaiting() || !self.violations().is_empty() || self.is_drafting() {
            return false;
        }
        self.state = RunState::Running;
        self.say_msg(
            Speaker::User,
            MessageKind::Info,
            msg!(CoreAgentMPlanApproved),
        );
        true
    }

    /// Rejects the plan. The run ends without traces, as with cancelling.
    pub fn reject(&mut self) -> bool {
        if !self.is_awaiting() {
            return false;
        }
        self.reset();
        true
    }

    /// Requests a new plan with a note. Empty notes do not count.
    pub fn revise(&mut self, note: &str) -> bool {
        let note = note.trim();
        if !self.is_awaiting() || note.is_empty() {
            return false;
        }
        self.plan_notes.push(note.to_string());
        self.say_msg(
            Speaker::User,
            MessageKind::Hint,
            msg!(CoreAgentMPlanNote, note = note),
        );
        // The agent works out a new plan; until it is ready, no approval is possible.
        self.agent_plan = None;
        true
    }

    /// The agent is still working out the plan: there is nothing to approve.
    pub fn is_drafting(&self) -> bool {
        self.is_awaiting() && self.agent_plan.is_none()
    }

    /// Answers the open question; afterwards the agent keeps working. Empty answers
    /// do not count.
    pub fn answer(&mut self, text: &str) -> bool {
        let text = text.trim();
        if !self.is_asking() || text.is_empty() {
            return false;
        }
        self.question = None;
        self.state = RunState::Running;
        self.say(Speaker::User, MessageKind::Answer, text.to_string());
        true
    }

    /// Sends the agent a hint. It takes it into account in the next step.
    /// Possible as long as the run works on the steps (also paused or waiting).
    pub fn steer(&mut self, text: &str) -> bool {
        let text = text.trim();
        if !self.is_working() || text.is_empty() {
            return false;
        }
        self.pending_hints.push(text.to_string());
        self.say(Speaker::User, MessageKind::Hint, text.to_string());
        true
    }

    /// Stops a running agent. An open question remains.
    pub fn pause(&mut self) -> bool {
        if !self.is_running() {
            return false;
        }
        self.state = RunState::Paused;
        self.say_msg(Speaker::User, MessageKind::Info, msg!(CoreAgentMPaused));
        true
    }

    /// Resumes a paused run.
    pub fn resume(&mut self) -> bool {
        if !self.is_paused() {
            return false;
        }
        self.state = RunState::Running;
        self.say_msg(Speaker::User, MessageKind::Info, msg!(CoreAgentMResumed));
        true
    }

    /// Resumes a stopped run. `extra` raises what stopped it: cents for the
    /// run's budget, minutes for the time limit. The workspace raises the monthly budget itself;
    /// here it is only the message. `false` if the run is not stopped.
    pub fn resume_after_stop(&mut self, extra: u64) -> bool {
        let Some(reason) = self.stop_reason() else {
            return false;
        };
        match reason {
            StopReason::RunBudget => {
                self.extra_budget_cents = self.extra_budget_cents.saturating_add(extra)
            }
            StopReason::Time => self.extra_minutes = self.extra_minutes.saturating_add(extra),
            StopReason::MonthBudget => {}
        }
        self.state = RunState::Running;
        let text = if extra == 0 {
            msg!(CoreAgentMResumed)
        } else {
            msg!(
                CoreAgentMResumedRaised,
                what = reason.raises_msg(),
                amount = reason.raise_label_msg(extra)
            )
        };
        self.say_msg(Speaker::User, MessageKind::Info, text);
        true
    }
}
