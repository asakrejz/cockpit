use super::texts::{comments_msg, files_msg, raw};
use super::*;
use cockpit_i18n::{ArgValue, Msg, msg};

type StepTexts = [(Msg, Msg); STEPS];

impl AgentRun {
    /// `#12`, or "pull request" as long as there is no number.
    pub(super) fn pr_label(&self) -> Msg {
        self.pr_number
            .map_or_else(|| msg!(CoreAgentPrFallback), |n| raw(format!("#{n}")))
    }

    /// Title and text of each step, matching kind and options of the run.
    pub fn steps(&self) -> Vec<Step> {
        let texts = match &self.kind {
            RunKind::Implement => self.implement_texts(),
            RunKind::AddressReview { review, comments } => {
                self.address_texts(review, comments.len())
            }
            RunKind::Diagnose { review, check } => self.diagnose_texts(review, check),
            // Real jobs report their own steps; this is only the fallback for the plan texts.
            RunKind::Followup { review, .. } | RunKind::ReviewPr { review } => {
                self.address_texts(review, 0)
            }
        };
        self.build_steps(texts)
    }

    fn tests_step(&self) -> (Msg, Msg) {
        if self.options.run_tests {
            (
                msg!(CoreAgentStepTitleTestsRun),
                msg!(CoreAgentStepTextTests),
            )
        } else {
            (
                msg!(CoreAgentStepTitleTestsSkipped),
                msg!(CoreAgentStepTextTestsOff),
            )
        }
    }

    pub(super) fn address_texts(&self, review: &PrKey, comments: usize) -> StepTexts {
        [
            (
                msg!(CoreAgentStepTitleCommentsRead),
                msg!(
                    CoreAgentStepTextCommentsRead,
                    comments = comments_msg(comments),
                    review = review.to_string()
                ),
            ),
            (msg!(CoreAgentStepTitleBranchCheckedOut), raw(self.branch())),
            (
                msg!(CoreAgentStepTitleChangesDone),
                msg!(
                    CoreAgentStepTextChanges,
                    files = files_msg(self.files.len())
                ),
            ),
            self.tests_step(),
            (
                msg!(CoreAgentStepTitlePushed),
                msg!(CoreAgentStepTextPushedReview, branch = self.branch()),
            ),
        ]
    }

    pub(super) fn diagnose_texts(&self, review: &PrKey, check: &str) -> StepTexts {
        let code = self.diagnosis.as_ref().is_none_or(Diagnosis::changes_code);
        [
            (
                msg!(CoreAgentStepTitleLogRead),
                msg!(
                    CoreAgentStepTextLogRead,
                    check = check,
                    review = review.to_string()
                ),
            ),
            (
                msg!(CoreAgentStepTitleCauseFound),
                msg!(CoreAgentStepTextCause),
            ),
            (
                msg!(CoreAgentStepTitleDiagnosisMade),
                msg!(CoreAgentStepTextDiagnosis),
            ),
            if code {
                (
                    msg!(CoreAgentStepTitleFixApplied),
                    msg!(CoreAgentStepTextFix),
                )
            } else {
                (msg!(CoreAgentStepTitleNoFix), msg!(CoreAgentStepTextNoFix))
            },
            if code {
                (
                    msg!(CoreAgentStepTitlePushedRestarted),
                    msg!(
                        CoreAgentStepTextPushedRestarted,
                        branch = self.branch(),
                        check = check
                    ),
                )
            } else {
                (
                    msg!(CoreAgentStepTitleCheckRestarted),
                    msg!(CoreAgentStepTextCheckRestarted, check = check),
                )
            },
        ]
    }

    pub(super) fn implement_texts(&self) -> StepTexts {
        let o = &self.options;
        let files = files_msg(self.files.len());
        let changes = if self.rules.has_path_rules() && self.violations().is_empty() {
            msg!(CoreAgentStepTextChangesRules, files = files)
        } else {
            msg!(CoreAgentStepTextChanges, files = files)
        };
        [
            (
                msg!(CoreAgentStepTitleTicketRead),
                msg!(CoreAgentStepTextTicketRead, ticket = &self.ticket_key),
            ),
            (
                msg!(CoreAgentStepTitleBranchCreated),
                msg!(
                    CoreAgentStepTextBranchFrom,
                    branch = self.branch(),
                    base = &o.base_branch
                ),
            ),
            (msg!(CoreAgentStepTitleChangesDone), changes),
            if !self.rules.required_tests.is_empty() {
                (
                    msg!(CoreAgentStepTitleRequiredTestsRun),
                    msg!(
                        CoreAgentStepTextRequired,
                        tests = self.rules.required_tests.join(", ")
                    ),
                )
            } else {
                self.tests_step()
            },
            (
                msg!(CoreAgentStepTitlePrCreated),
                if o.draft_pr {
                    msg!(CoreAgentStepTextPrDraft, pr = self.pr_label())
                } else {
                    msg!(CoreAgentStepTextPr, pr = self.pr_label())
                },
            ),
        ]
    }

    pub(super) fn build_steps(&self, texts: StepTexts) -> Vec<Step> {
        texts
            .into_iter()
            .enumerate()
            .map(|(i, (title, text))| {
                if i == 0 && self.is_awaiting() {
                    return Step::new(
                        msg!(CoreAgentStepTitlePlanReady),
                        if self.violations().is_empty() {
                            msg!(CoreAgentStepTextPlanWait)
                        } else {
                            msg!(CoreAgentStepTextPlanViolation)
                        },
                        StepState::Active,
                        None,
                    );
                }
                if i == self.completed && self.is_diagnosed() {
                    return Step::new(
                        msg!(CoreAgentStepTitleDiagnosisReady),
                        msg!(CoreAgentStepTextDiagnosisWait),
                        StepState::Active,
                        None,
                    );
                }
                if i == self.completed && self.is_asking() {
                    return Step::new(
                        msg!(CoreAgentStepTitleQuestion),
                        msg!(CoreAgentStepTextAnswerWait),
                        StepState::Active,
                        None,
                    );
                }
                if i == self.completed && self.is_paused() {
                    return Step::new(
                        title,
                        msg!(CoreAgentStepTextPaused),
                        StepState::Active,
                        None,
                    );
                }
                if i == self.completed
                    && let Some(reason) = self.stop_reason()
                {
                    return Step::new(
                        title,
                        msg!(CoreAgentStepTextHalted, reason = reason.label_msg()),
                        StepState::Active,
                        None,
                    );
                }
                // If the user answered a question at this step, it is noted there.
                let text = match self
                    .messages
                    .iter()
                    .find(|m| m.kind == MessageKind::Answer && m.step == i)
                {
                    Some(answer) => msg!(
                        CoreAgentStepTextWithAnswer,
                        text = ArgValue::from(text),
                        answer = &answer.text
                    ),
                    None => text,
                };
                Step::new(
                    title,
                    text,
                    if i < self.completed {
                        StepState::Done
                    } else if i == self.completed && self.is_working() {
                        StepState::Active
                    } else {
                        StepState::Pending
                    },
                    if i < self.completed {
                        self.step_usage.get(i).copied()
                    } else {
                        None
                    },
                )
            })
            .collect()
    }
}
