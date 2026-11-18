use super::texts::{bullet, comments_msg, raw};
use super::*;
use cockpit_i18n::{Lang, Msg, msg};

impl AgentRun {
    /// The log of a real run: what the agent and you said, plus the consumption.
    fn real_log(&self) -> Vec<Msg> {
        let mut lines: Vec<Msg> = self
            .messages
            .iter()
            .map(|m| {
                let text = m.msg.clone().unwrap_or_else(|| raw(m.text.clone()));
                bullet(match m.from {
                    Speaker::Agent => msg!(CoreAgentLogFromAgent, text = text),
                    Speaker::User => msg!(CoreAgentLogFromUser, text = text),
                })
            })
            .collect();
        let usage = self.usage();
        if !usage.is_empty() {
            lines.push(bullet(msg!(CoreAgentLogUsage, usage = usage.line_msg())));
        }
        if lines.is_empty() {
            return vec![msg!(CoreAgentLogNone)];
        }
        lines
    }

    fn plan_made_lines(&self, lines: &mut Vec<Msg>, plan: &Plan) {
        lines.push(bullet(msg!(
            CoreAgentLogPlanMade,
            steps = plan.steps.len(),
            files = plan.files.len()
        )));
    }

    fn plan_state_lines(&self, lines: &mut Vec<Msg>) {
        for note in &self.plan_notes {
            lines.push(bullet(msg!(CoreAgentLogNewPlan, note = note)));
        }
        lines.push(bullet(if self.is_awaiting() {
            msg!(CoreAgentLogWaitApproval)
        } else {
            msg!(CoreAgentLogPlanApproved)
        }));
    }

    /// Log lines of a step of "implement review comments".
    pub(super) fn address_log(
        &self,
        lines: &mut Vec<Msg>,
        step: usize,
        review: &PrKey,
        comments: usize,
    ) {
        let o = &self.options;
        let hint = o.hint.trim();
        match step {
            0 => {
                lines.push(bullet(msg!(
                    CoreAgentLogReadComments,
                    comments = comments_msg(comments),
                    review = review.to_string()
                )));
                if !hint.is_empty() {
                    lines.push(bullet(msg!(CoreAgentLogHint, hint = hint)));
                }
                if let Some(plan) = self.plan() {
                    self.plan_made_lines(lines, &plan);
                    self.plan_state_lines(lines);
                }
            }
            1 => lines.push(bullet(raw(format!("git switch {}", self.branch())))),
            2 => lines.push(bullet(msg!(
                CoreAgentLogEdit,
                files = self.files.join(", ")
            ))),
            3 if o.run_tests => lines.push(bullet(msg!(CoreAgentLogTestsRun))),
            3 => lines.push(bullet(msg!(CoreAgentLogTestsSkipped))),
            _ => lines.push(bullet(msg!(CoreAgentLogPushNoPr, branch = self.branch()))),
        }
    }

    /// Log lines of a step of "investigate check".
    pub(super) fn diagnose_log(
        &self,
        lines: &mut Vec<Msg>,
        step: usize,
        review: &PrKey,
        check: &str,
    ) {
        let code = self.diagnosis.as_ref().is_none_or(Diagnosis::changes_code);
        lines.push(bullet(match step {
            0 => msg!(
                CoreAgentLogReadCheck,
                check = check,
                review = review.to_string()
            ),
            1 => msg!(CoreAgentLogFindError),
            2 => match &self.diagnosis {
                Some(d) => msg!(CoreAgentLogDiagnosis, cause = d.cause.clone()),
                None => msg!(CoreAgentLogDiagnosisMade),
            },
            3 if code => msg!(CoreAgentLogApplyFix),
            3 => msg!(CoreAgentLogNoFix),
            _ if code => msg!(
                CoreAgentLogPushRestart,
                branch = self.branch(),
                check = check
            ),
            _ => msg!(CoreAgentLogRestart, check = check),
        }));
    }

    /// Log up to and including the step the agent is currently working on, as messages.
    pub fn log_msgs(&self) -> Vec<Msg> {
        if self.real || self.ended.is_some() {
            return self.real_log();
        }
        let shown = if self.is_awaiting() {
            1
        } else if self.is_diagnosed() {
            self.completed
        } else if self.is_working() {
            self.completed + 1
        } else {
            self.completed
        }
        .min(STEPS);
        if shown == 0 {
            return vec![msg!(CoreAgentLogNoneStart)];
        }
        let o = &self.options;
        let hint = o.hint.trim();
        let mut lines = Vec::new();
        for step in 0..shown {
            match (&self.kind, step) {
                (RunKind::AddressReview { review, comments }, _) => {
                    self.address_log(&mut lines, step, review, comments.len());
                }
                (RunKind::Diagnose { review, check }, _) => {
                    self.diagnose_log(&mut lines, step, review, check);
                }
                (RunKind::Followup { review, .. } | RunKind::ReviewPr { review }, _) => {
                    self.address_log(&mut lines, step, review, 0);
                }
                (RunKind::Implement, 0) => {
                    lines.push(bullet(msg!(
                        CoreAgentLogReadTicket,
                        ticket = &self.ticket_key,
                        comments = comments_msg(self.comments)
                    )));
                    if !hint.is_empty() {
                        lines.push(bullet(msg!(CoreAgentLogHint, hint = hint)));
                    }
                    for rule in self.rules.lines_msg() {
                        lines.push(bullet(msg!(CoreAgentLogRule, rule = rule)));
                    }
                    if let Some(plan) = self.plan() {
                        self.plan_made_lines(&mut lines, &plan);
                        for v in &plan.violations {
                            lines
                                .push(bullet(msg!(CoreAgentLogViolation, reason = v.reason_msg())));
                        }
                        self.plan_state_lines(&mut lines);
                    }
                }
                (RunKind::Implement, 1) => {
                    lines.push(bullet(raw(format!("git switch -c {}", self.branch()))))
                }
                (RunKind::Implement, 2) => lines.push(bullet(msg!(
                    CoreAgentLogEdit,
                    files = self.files.join(", ")
                ))),
                (RunKind::Implement, 3) if !self.rules.required_tests.is_empty() => {
                    for cmd in &self.rules.required_tests {
                        lines.push(bullet(msg!(CoreAgentLogRequiredTest, cmd = cmd)));
                    }
                }
                (RunKind::Implement, 3) if o.run_tests => {
                    lines.push(bullet(msg!(CoreAgentLogTestsRun)))
                }
                (RunKind::Implement, 3) => lines.push(bullet(msg!(CoreAgentLogTestsSkipped))),
                (RunKind::Implement, _) => lines.push(bullet(if o.draft_pr {
                    msg!(CoreAgentLogPrCreatedDraft, pr = self.pr_label())
                } else {
                    msg!(CoreAgentLogPrCreated, pr = self.pr_label())
                })),
            }
            // Questions, answers and acknowledged hints of this step.
            for m in self.messages.iter().filter(|m| m.step == step) {
                let text = m.msg.clone().unwrap_or_else(|| raw(m.text.clone()));
                match m.kind {
                    MessageKind::Question => {
                        lines.push(bullet(msg!(CoreAgentLogQuestion, text = text)))
                    }
                    MessageKind::Answer => {
                        lines.push(bullet(msg!(CoreAgentLogAnswer, text = text)))
                    }
                    MessageKind::Ack => lines.push(bullet(text)),
                    _ => {}
                }
            }
        }
        if self.is_diagnosed() {
            lines.push(bullet(msg!(CoreAgentLogWaitFix)));
        } else if self.is_asking() {
            lines.push(bullet(msg!(CoreAgentLogWaitAnswer)));
        } else if self.is_paused() {
            lines.push(bullet(msg!(CoreAgentLogPaused)));
        } else if let Some(reason) = self.stop_reason() {
            lines.push(bullet(msg!(
                CoreAgentLogHalted,
                reason = reason.label_msg()
            )));
        }
        lines
    }

    /// The log in `lang`, one line per entry.
    pub fn log_in(&self, lang: Lang) -> String {
        self.log_msgs()
            .iter()
            .map(|m| m.render(lang))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The log in German (plain-text readers, tests).
    pub fn log(&self) -> String {
        self.log_in(Lang::De)
    }
}
