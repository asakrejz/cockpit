use super::*;
use cockpit_i18n::msg;

impl AgentRun {
    /// Takes a report of the real agent. `None` if it does not fit in the current state
    /// (the run is not active, the plan is not expected …): then nothing changes.
    /// The reports are the only way the container changes the run.
    pub fn report(
        &mut self,
        report: AgentReport,
        limits: &AgentLimits,
        month_spent_cents: u64,
    ) -> Option<ReportEffect> {
        if !self.is_active() {
            return None;
        }
        let mut effect = ReportEffect::default();
        match report {
            AgentReport::Started { detail } => {
                self.real = true;
                // A ready-to-start run carries placeholder names. The container reports the
                // files it really changes (`Files`, `Repos`), so an implementation starts empty.
                if self.kind == RunKind::Implement {
                    self.files.clear();
                }
                effect
                    .notes
                    .push(msg!(CoreAgentNContainerStarted, label = self.label_msg()));
                self.say(Speaker::Agent, MessageKind::Info, detail);
            }
            AgentReport::Plan {
                summary,
                files,
                steps,
            } => {
                if !self.is_awaiting() {
                    return None;
                }
                self.real = true;
                self.files = files.clone();
                self.agent_plan = Some(AgentPlan {
                    summary,
                    files,
                    steps,
                });
                self.say_msg(Speaker::Agent, MessageKind::Info, msg!(CoreAgentMPlanReady));
                effect
                    .notes
                    .push(msg!(CoreAgentNPlanPresented, label = self.label_msg()));
            }
            AgentReport::Step {
                title,
                text,
                input_tokens,
                output_tokens,
                millis,
            } => {
                if !(self.is_working() || self.is_awaiting()) {
                    return None;
                }
                let usage = Usage::priced(input_tokens, output_tokens, millis, &limits.pricing);
                self.record_usage(usage);
                effect.usage = Some(usage);
                let line = if text.trim().is_empty() {
                    title.clone()
                } else {
                    format!("{title}: {text}")
                };
                self.say(Speaker::Agent, MessageKind::Info, line);
                if self.is_working() {
                    // The last step belongs to the result; only `Done` completes the run.
                    if self.completed + 1 < STEPS {
                        self.completed += 1;
                    }
                    effect.notes.push(msg!(
                        CoreAgentNStep,
                        label = self.label_msg(),
                        title = &title
                    ));
                    if self.is_running()
                        && let Some(reason) =
                            limits.reached(self, month_spent_cents.saturating_add(usage.cost_cents))
                    {
                        self.state = RunState::Stopped(reason);
                        let detail = limits.describe_msg(
                            reason,
                            self,
                            month_spent_cents.saturating_add(usage.cost_cents),
                        );
                        self.say_msg(
                            Speaker::Agent,
                            MessageKind::Info,
                            msg!(
                                CoreAgentMHalted,
                                reason = reason.label_msg(),
                                detail = detail,
                                what = reason.raises_msg()
                            ),
                        );
                        effect.notes.push(msg!(
                            CoreAgentNHalted,
                            label = self.label_msg(),
                            reason = reason.label_msg()
                        ));
                        effect.stopped = Some(reason);
                    }
                }
            }
            AgentReport::Files { files } => {
                if !self.is_working() {
                    return None;
                }
                self.files = files;
            }
            AgentReport::Question { text, suggestions } => {
                if !self.is_running() || text.trim().is_empty() {
                    return None;
                }
                self.asked = true;
                self.state = RunState::AwaitingAnswer;
                self.say(Speaker::Agent, MessageKind::Question, text.clone());
                self.question = Some(Question { text, suggestions });
                effect.asking = true;
                effect
                    .notes
                    .push(msg!(CoreAgentNQuestion, label = self.label_msg()));
            }
            // Booked on the review by the workspace, which has the diff. Only a running
            // review run takes them.
            AgentReport::ReviewComment { .. } | AgentReport::ReviewSummary { .. } => {
                if !self.is_running() || !matches!(self.kind, RunKind::ReviewPr { .. }) {
                    return None;
                }
            }
            AgentReport::HintsDelivered { count } => {
                let count = count.min(self.pending_hints.len());
                for hint in self.pending_hints.drain(..count).collect::<Vec<_>>() {
                    self.say_msg(
                        Speaker::Agent,
                        MessageKind::Ack,
                        msg!(CoreAgentMHintReceived, hint = hint),
                    );
                }
            }
            AgentReport::Repos { repos } => {
                if !self.is_working() {
                    return None;
                }
                self.book_repos(&repos);
            }
            AgentReport::Done { summary } => {
                if !self.is_working() {
                    return None;
                }
                self.state = RunState::Done;
                self.completed = STEPS;
                self.summary = summary.clone();
                self.question = None;
                self.say(Speaker::Agent, MessageKind::Info, summary);
                effect.finished = true;
            }
            AgentReport::Failed { reason } => {
                let label = self.label_msg();
                self.reset();
                self.ended = Some(reason.clone());
                self.say_msg(
                    Speaker::Agent,
                    MessageKind::Info,
                    msg!(CoreAgentMFailed, reason = &reason),
                );
                effect.failed = true;
                effect
                    .notes
                    .push(msg!(CoreAgentNFailed, label = label, reason = &reason));
            }
        }
        Some(effect)
    }
}
