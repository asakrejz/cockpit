use super::texts::{comments_msg, files_msg, raw};
use super::*;
use cockpit_i18n::{Msg, msg};

impl AgentRun {
    /// The current plan. Before the start there is none.
    pub fn plan(&self) -> Option<Plan> {
        if !self.planned {
            return None;
        }
        if let Some(p) = &self.agent_plan {
            return Some(Plan::new(
                raw(p.summary.clone()),
                p.files.clone(),
                p.steps.iter().cloned().map(raw).collect(),
                self.rules.lines_msg(),
                self.violations(),
            ));
        }
        if let RunKind::AddressReview { review, comments } = &self.kind {
            return Some(self.address_plan(review, comments.len()));
        }
        let o = &self.options;
        let tests = tests_part(o.run_tests);
        let mut summary = msg!(
            CoreAgentPlanSummary,
            ticket = &self.ticket_key,
            files = files_msg(self.files.len()),
            tests = tests
        );
        summary = self.with_hint_and_revisions(summary);
        let mut steps = vec![
            msg!(
                CoreAgentPlanRead,
                ticket = &self.ticket_key,
                n = self.comments
            ),
            msg!(
                CoreAgentPlanBranch,
                branch = self.branch(),
                base = &o.base_branch
            ),
            msg!(CoreAgentPlanEdit, files = self.files.join(", ")),
        ];
        self.push_note_and_test_steps(&mut steps);
        steps.push(if o.draft_pr {
            msg!(CoreAgentPlanPrDraft)
        } else {
            msg!(CoreAgentPlanPr)
        });
        Some(Plan::new(
            summary,
            self.files.clone(),
            steps,
            self.rules.lines_msg(),
            self.violations(),
        ))
    }

    /// Adds " Your hint: ..." and " Revised (2x)." to the summary.
    fn with_hint_and_revisions(&self, mut summary: Msg) -> Msg {
        let hint = self.options.hint.trim();
        if !hint.is_empty() {
            summary = msg!(CoreAgentPlanWithHint, summary = summary, hint = hint);
        }
        if !self.plan_notes.is_empty() {
            summary = msg!(
                CoreAgentPlanRevised,
                summary = summary,
                n = self.plan_notes.len()
            );
        }
        summary
    }

    /// The notes of the user and the test step, which are the same for every kind of plan.
    fn push_note_and_test_steps(&self, steps: &mut Vec<Msg>) {
        for note in &self.plan_notes {
            steps.push(msg!(CoreAgentPlanNote, note = note));
        }
        steps.push(if !self.rules.required_tests.is_empty() {
            msg!(
                CoreAgentPlanRequired,
                tests = self.rules.required_tests.join(", ")
            )
        } else if self.options.run_tests {
            msg!(CoreAgentPlanTests)
        } else {
            msg!(CoreAgentPlanTestsSkip)
        });
    }

    /// Plan for "implement review comments": no new branch, no new pull request.
    pub(super) fn address_plan(&self, review: &PrKey, comments: usize) -> Plan {
        let branch = self.branch();
        let summary = self.with_hint_and_revisions(msg!(
            CoreAgentPlanAddressSummary,
            comments = comments_msg(comments),
            review = review.to_string(),
            files = files_msg(self.files.len()),
            tests = tests_part(self.options.run_tests),
            branch = &branch
        ));
        let mut steps = vec![
            msg!(
                CoreAgentPlanAddressRead,
                comments = comments_msg(comments),
                review = review.to_string()
            ),
            msg!(CoreAgentPlanAddressCheckout, branch = &branch),
            msg!(CoreAgentPlanEdit, files = self.files.join(", ")),
        ];
        self.push_note_and_test_steps(&mut steps);
        steps.push(msg!(CoreAgentPlanAddressPush, branch = &branch));
        Plan::new(
            summary,
            self.files.clone(),
            steps,
            self.rules.lines_msg(),
            self.violations(),
        )
    }
}

/// ", run tests" or " (without tests)".
fn tests_part(run_tests: bool) -> Msg {
    if run_tests {
        msg!(CoreAgentPlanWithTests)
    } else {
        msg!(CoreAgentPlanWithoutTests)
    }
}
