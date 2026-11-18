use cockpit_i18n::msg;

use super::*;

impl Workspace {
    pub(super) fn perform_review(&mut self, action: Action) -> Result<Vec<Note>, ActionError> {
        match action {
            Action::Decide {
                review,
                suggestion,
                decision,
            } => {
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
            Action::EditSuggestion {
                review,
                suggestion,
                text,
            } => {
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
            Action::AddLineComment {
                review,
                file,
                line,
                text,
                suggestion,
            } => {
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
            Action::EditLineComment {
                review,
                comment,
                text,
                suggestion,
            } => {
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
            Action::DeleteLineComment { review, comment } => {
                self.need_open_review(&review)?;
                if !self.delete_line_comment(&review, &comment) {
                    return Err(ActionError::UnknownComment(comment));
                }
                Ok(vec![Note::coded(
                    Target::Review(review.clone()),
                    msg!(CoreActionLogCommentDeleted, review = review.to_string()),
                )])
            }
            Action::SetVerdict { review, verdict } => {
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
            Action::RequestAgentReview { review } => {
                self.need_open_review(&review)?;
                let count = self.request_agent_review(&review);
                let result = match count {
                    0 => msg!(CoreActionAgentReviewNone),
                    n => msg!(CoreActionAgentReviewFound, comments = labels::comments(n)),
                };
                Ok(vec![
                    Note::coded(
                        Target::Review(review.clone()),
                        msg!(
                            CoreActionLogAgentReviewRequested,
                            review = review.to_string()
                        ),
                    ),
                    Note::coded(
                        Target::Review(review.clone()),
                        msg!(
                            CoreActionLogAgentReviewDone,
                            review = review.to_string(),
                            result = result
                        ),
                    )
                    .by(Actor::Agent),
                ])
            }
            Action::SendReview { review, comment } => {
                let count = self.send_review(&review, &comment).map_err(|e| match e {
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
            Action::RestartCheck { review, check } => {
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
            Action::AdvanceChecks => {
                if self.running_checks() == 0 {
                    return Err(ActionError::NothingToDo);
                }
                Ok(self
                    .advance_checks()
                    .into_iter()
                    .map(|(number, check, status)| {
                        let (check, review) = (check, number.to_string());
                        let text = if status == Checks::Passed {
                            msg!(CoreActionLogCheckPassed, check = check, review = review)
                        } else {
                            msg!(
                                CoreActionLogCheckFailedAgain,
                                check = check,
                                review = review
                            )
                        };
                        Note::coded(Target::Review(number.clone()), text).by(Actor::System)
                    })
                    .collect())
            }
            Action::MergePr { review, method } => {
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
            _ => unreachable!("review: dispatched by perform"),
        }
    }
}
