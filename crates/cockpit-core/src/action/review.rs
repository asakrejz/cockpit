//! Handlers of the review actions: decisions on suggestions, own line comments, verdict, sending,
//! restarting a check and merging.

use cockpit_i18n::msg;

use super::*;

impl Workspace {
    pub(super) fn perform_decide(
        &mut self,
        review: PrKey,
        suggestion: String,
        decision: Option<Decision>,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        if !self.decide(&review, &suggestion, decision) {
            return Err(ActionError::UnknownSuggestion(suggestion));
        }
        let (suggestion, review_text) = (suggestion, review.to_string());
        let text = match decision {
            Some(Decision::Approved) => {
                msg!(
                    CoreActionLogDecisionApproved,
                    suggestion = suggestion,
                    review = review_text
                )
            }
            Some(Decision::Declined) => {
                msg!(
                    CoreActionLogDecisionDeclined,
                    suggestion = suggestion,
                    review = review_text
                )
            }
            None => {
                msg!(
                    CoreActionLogDecisionWithdrawn,
                    suggestion = suggestion,
                    review = review_text
                )
            }
        };
        Ok(vec![Note::coded(Target::Review(review.clone()), text)])
    }

    pub(super) fn perform_edit_suggestion(
        &mut self,
        review: PrKey,
        suggestion: String,
        text: String,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        let known = self
            .review(&review)
            .is_some_and(|r| r.suggestions.iter().any(|s| s.id == suggestion));
        if !known {
            return Err(ActionError::UnknownSuggestion(suggestion));
        }
        if !self.edit_suggestion(&review, &suggestion, &text) {
            return Err(ActionError::InvalidInput);
        }
        Ok(vec![Note::coded(
            Target::Review(review.clone()),
            msg!(
                CoreActionLogSuggestionEdited,
                suggestion = suggestion,
                review = review.to_string()
            ),
        )])
    }

    pub(super) fn perform_add_line_comment(
        &mut self,
        review: PrKey,
        file: String,
        line: usize,
        text: String,
        suggestion: Option<String>,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        if self
            .add_line_comment(&review, &file, line, &text, suggestion.as_deref())
            .is_none()
        {
            return Err(ActionError::InvalidInput);
        }
        let (file, line, review_text) = (file, line + 1, review.to_string());
        let text = if suggestion.is_some() {
            msg!(
                CoreActionLogCommentAddedSuggestion,
                file = file,
                line = line,
                review = review_text
            )
        } else {
            msg!(
                CoreActionLogCommentAdded,
                file = file,
                line = line,
                review = review_text
            )
        };
        Ok(vec![Note::coded(Target::Review(review.clone()), text)])
    }

    pub(super) fn perform_edit_line_comment(
        &mut self,
        review: PrKey,
        comment: String,
        text: String,
        suggestion: Option<String>,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        let known = self
            .review(&review)
            .is_some_and(|r| r.line_comments.iter().any(|c| c.id == comment));
        if !known {
            return Err(ActionError::UnknownComment(comment));
        }
        if !self.edit_line_comment(&review, &comment, &text, suggestion.as_deref()) {
            return Err(ActionError::InvalidInput);
        }
        Ok(vec![Note::coded(
            Target::Review(review.clone()),
            msg!(CoreActionLogCommentEdited, review = review.to_string()),
        )])
    }

    pub(super) fn perform_delete_line_comment(
        &mut self,
        review: PrKey,
        comment: String,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        if !self.delete_line_comment(&review, &comment) {
            return Err(ActionError::UnknownComment(comment));
        }
        Ok(vec![Note::coded(
            Target::Review(review.clone()),
            msg!(CoreActionLogCommentDeleted, review = review.to_string()),
        )])
    }

    pub(super) fn perform_set_verdict(
        &mut self,
        review: PrKey,
        verdict: Verdict,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        if self.review(&review).is_some_and(|r| r.verdict == verdict) {
            return Err(ActionError::Unchanged);
        }
        self.set_verdict(&review, verdict);
        Ok(vec![Note::coded(
            Target::Review(review.clone()),
            msg!(
                CoreActionLogVerdictSet,
                review = review.to_string(),
                verdict = labels::verdict(verdict)
            ),
        )])
    }

    pub(super) fn perform_send_review(
        &mut self,
        review: PrKey,
        comment: String,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        let count = self
            .send_review(&review, &comment, at_ms)
            .map_err(|e| match e {
                SendError::UnknownReview => ActionError::UnknownReview(review.clone()),
                e => ActionError::Send(e),
            })?;
        Ok(vec![Note::coded(
            Target::Review(review.clone()),
            msg!(
                CoreActionLogReviewSent,
                review = review.to_string(),
                comments = labels::comments(count)
            ),
        )])
    }

    pub(super) fn perform_restart_check(
        &mut self,
        review: PrKey,
        check: String,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_open_review(&review)?;
        let known = self
            .review(&review)
            .is_some_and(|r| r.check_runs.iter().any(|c| c.name == check));
        if !known {
            return Err(ActionError::UnknownCheck(check));
        }
        if !self.restart_check(&review, &check) {
            return Err(ActionError::NothingToDo);
        }
        Ok(vec![Note::coded(
            Target::Review(review.clone()),
            msg!(
                CoreActionLogCheckRestarted,
                check = check,
                review = review.to_string()
            ),
        )])
    }

    pub(super) fn perform_merge_pr(
        &mut self,
        review: PrKey,
        method: MergeMethod,
    ) -> Result<Vec<Note>, ActionError> {
        self.need_review(&review)?;
        let changes = self
            .merge_pr(&review, method)
            .map_err(ActionError::MergeBlocked)?;
        let mut notes = vec![Note::coded(
            Target::Review(review.clone()),
            msg!(
                CoreActionLogMerged,
                review = review.to_string(),
                method = labels::method(method)
            ),
        )];
        notes.extend(status_notes(changes));
        Ok(notes)
    }
}
