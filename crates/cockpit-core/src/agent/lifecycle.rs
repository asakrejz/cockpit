use super::*;

impl AgentRun {
    pub fn start(&mut self, mut options: AgentOptions, pr_number: u32) {
        // The rules of the repo come first: required tests cannot be switched off, and whoever
        // violates the path rules or whose repo demands it always presents a plan.
        if !self.rules.required_tests.is_empty() {
            options.run_tests = true;
        }
        if self.rules.plan_required || !self.violations().is_empty() {
            options.plan_first = true;
        }
        // An investigation only changes something after your approval of the fix, a plan would be
        // redundant. Follow-up jobs do not create a new pull request.
        let investigate = matches!(self.kind, RunKind::Diagnose { .. });
        self.planned = options.plan_first && !investigate;
        self.state = if self.planned {
            RunState::AwaitingApproval
        } else {
            RunState::Running
        };
        self.options = options;
        self.completed = 0;
        self.pr_number = Some(self.kind.review().unwrap_or(pr_number));
        self.plan_notes.clear();
        self.diagnosis = None;
        self.agent_plan = None;
        self.ended = None;
        self.summary.clear();
        self.clear_usage();
        self.clear_conversation();
        self.say(
            Speaker::Agent,
            MessageKind::Info,
            format!("Auftrag für {} angenommen.", self.label()),
        );
        let hint = self.options.hint.trim().to_string();
        if !hint.is_empty() {
            self.say(Speaker::User, MessageKind::Hint, hint);
        }
        if self.planned {
            self.say(
                Speaker::Agent,
                MessageKind::Info,
                "Ich habe einen Plan vorgelegt und warte auf deine Freigabe.".into(),
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
        });
    }

    /// Cancels or resets: the run is ready to start again afterwards.
    pub fn reset(&mut self) {
        self.state = RunState::Idle;
        self.completed = 0;
        self.pr_number = self.kind.review();
        self.plan_notes.clear();
        self.planned = false;
        self.diagnosis = None;
        self.agent_plan = None;
        self.summary.clear();
        self.real = false;
        self.clear_usage();
        self.clear_conversation();
    }

    /// Adopts the fix of the diagnosis: the agent applies it, pushes and restarts the check.
    /// `false` if no diagnosis is waiting.
    pub fn accept_fix(&mut self) -> bool {
        if !self.is_diagnosed() {
            return false;
        }
        self.state = RunState::Running;
        self.say(Speaker::User, MessageKind::Info, "Fix übernommen.".into());
        true
    }

    /// Approves the plan: from now on the agent works. `false` if there is nothing to approve.
    pub fn approve(&mut self) -> bool {
        // A plan that touches forbidden paths cannot be approved.
        if !self.is_awaiting() || !self.violations().is_empty() || self.is_drafting() {
            return false;
        }
        self.state = RunState::Running;
        self.say(Speaker::User, MessageKind::Info, "Plan freigegeben.".into());
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
        self.say(
            Speaker::User,
            MessageKind::Hint,
            format!("Anmerkung zum Plan: {note}"),
        );
        // The real agent works out a new plan; until it is ready, no approval is possible.
        if self.real {
            self.agent_plan = None;
        }
        true
    }

    /// The real agent is still working out the plan: there is nothing to approve.
    pub fn is_drafting(&self) -> bool {
        self.is_awaiting() && self.real && self.agent_plan.is_none()
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
        self.say(Speaker::User, MessageKind::Info, "Lauf pausiert.".into());
        true
    }

    /// Resumes a paused run.
    pub fn resume(&mut self) -> bool {
        if !self.is_paused() {
            return false;
        }
        self.state = RunState::Running;
        self.say(Speaker::User, MessageKind::Info, "Lauf fortgesetzt.".into());
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
            "Lauf fortgesetzt.".to_string()
        } else {
            format!(
                "Lauf fortgesetzt – {} um {} erhöht.",
                reason.raises(),
                reason.raise_label(extra)
            )
        };
        self.say(Speaker::User, MessageKind::Info, text);
        true
    }

    /// Completes the current step. Returns `true` if the run is
    /// finished with it.
    pub fn advance(&mut self) -> bool {
        self.tick() == Tick::Finished
    }

    /// A tick: the agent takes waiting hints into account, perhaps asks a
    /// question and otherwise completes the current step.
    pub fn tick(&mut self) -> Tick {
        self.tick_with(&AgentLimits::unlimited(), 0)
    }

    /// Like [`AgentRun::tick`], with limits. If a limit is reached before the step, the run
    /// stops (`Stopped`) and does nothing. `month_spent_cents` is what was spent in the month across all runs.
    /// The step's consumption is calculated by the tick according to the prices of the limits.
    pub fn tick_with(&mut self, limits: &AgentLimits, month_spent_cents: u64) -> Tick {
        if !self.is_running() || self.real {
            return Tick::Idle;
        }
        if let Some(reason) = limits.reached(self, month_spent_cents) {
            self.state = RunState::Stopped(reason);
            let detail = limits.describe(reason, self, month_spent_cents);
            self.say(
                Speaker::Agent,
                MessageKind::Info,
                format!(
                    "Angehalten: {}. {detail} Erhöhe {} und setze den Lauf fort – oder brich ihn ab.",
                    reason.label(),
                    reason.raises()
                ),
            );
            return Tick::Stopped;
        }
        for hint in std::mem::take(&mut self.pending_hints) {
            self.say(
                Speaker::Agent,
                MessageKind::Ack,
                format!("Hinweis erhalten: {hint}"),
            );
        }
        // The follow-up question is hard-wired in the demo (see `demo::agent_question`);
        // a backend would get it from the real agent.
        if !self.asked
            && let Some((step, question)) = crate::demo::agent_question(&self.key())
            && step == self.completed
        {
            self.asked = true;
            self.state = RunState::AwaitingAnswer;
            self.say(Speaker::Agent, MessageKind::Question, question.text.clone());
            self.question = Some(question);
            return Tick::Asked;
        }
        let done = self.steps()[self.completed.min(STEPS - 1)].clone();
        self.say(
            Speaker::Agent,
            MessageKind::Info,
            format!("{}: {}", done.title, done.text),
        );
        // Consumption of the step: made-up but fixed values in the demo.
        let (input, output, millis) = crate::demo::step_tokens(&self.ticket_key, self.completed);
        self.record_usage(Usage::priced(input, output, millis, &limits.pricing));
        self.completed += 1;
        if self.completed == STEPS {
            self.state = RunState::Done;
            Tick::Finished
        } else if self.completed == DIAGNOSED_AT && matches!(self.kind, RunKind::Diagnose { .. }) {
            // The diagnosis is ready. Until your decision the branch stays untouched.
            self.state = RunState::AwaitingFix;
            self.say(
                Speaker::Agent,
                MessageKind::Info,
                "Die Diagnose liegt vor. Ich warte, ob ich den Fix übernehmen soll.".into(),
            );
            Tick::Diagnosed
        } else {
            Tick::Stepped
        }
    }
}
