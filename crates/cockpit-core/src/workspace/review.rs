use super::*;

impl Workspace {
    // ---------- Review ----------

    pub fn decide(
        &mut self,
        number: impl Into<PrKey>,
        suggestion: &str,
        decision: Option<Decision>,
    ) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.decide(suggestion, decision))
    }

    pub fn edit_suggestion(
        &mut self,
        number: impl Into<PrKey>,
        suggestion: &str,
        text: &str,
    ) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.edit(suggestion, text))
    }

    /// Own comment on a diff line. Returns the ID of the comment.
    pub fn add_line_comment(
        &mut self,
        number: impl Into<PrKey>,
        file: &str,
        line: usize,
        text: &str,
        suggestion: Option<&str>,
    ) -> Option<String> {
        self.review_mut(number)?
            .add_line_comment(file, line, text, suggestion)
    }

    pub fn edit_line_comment(
        &mut self,
        number: impl Into<PrKey>,
        comment: &str,
        text: &str,
        suggestion: Option<&str>,
    ) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.edit_line_comment(comment, text, suggestion))
    }

    pub fn delete_line_comment(&mut self, number: impl Into<PrKey>, comment: &str) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.delete_line_comment(comment))
    }

    pub fn set_verdict(&mut self, number: impl Into<PrKey>, verdict: Verdict) {
        if let Some(r) = self.review_mut(number) {
            r.set_verdict(verdict);
        }
    }

    /// The entry that stands for this pull request in the inbox.
    pub(super) fn pr_item_mut(&mut self, key: impl Into<PrKey>) -> Option<&mut Item> {
        let key = key.into();
        self.items.iter_mut().find(|i| {
            i.pr.as_ref()
                .is_some_and(|p| key.matches(&p.repo, p.number))
        })
    }

    /// Sends the review. The inbox entry gets a summary as
    /// an own comment and counts as done. Returns the number of comments.
    pub fn send_review(
        &mut self,
        number: impl Into<PrKey>,
        own_comment: &str,
        now_ms: u64,
    ) -> Result<usize, SendError> {
        let key = number.into();
        let review = self.review_mut(&key).ok_or(SendError::UnknownReview)?;
        let verdict = review.verdict;
        let sent = review.send()?.len();

        // The comment on the entry says what was sent; the own text follows in a line of its own.
        let own_comment = own_comment.trim();
        let note = if own_comment.is_empty() {
            String::new()
        } else {
            format!("\n{own_comment}")
        };
        let sent_msg = cockpit_i18n::msg!(
            CoreWorkspaceReviewSent,
            verdict = verdict.label_msg(),
            comments = crate::action::labels::comments(sent),
            note = note
        );
        if let Some(item) = self.pr_item_mut(&key) {
            item.comments.push(Comment {
                own: true,
                text_msg: Some(sent_msg),
                ..Comment::at(Person::me(), now_ms, "")
            });
            item.clear_note();
            item.read = true;
            // The review is submitted; the request is answered.
            if item.reason == ReasonKind::ReviewRequested {
                item.reason = ReasonKind::Comment;
            }
        }
        Ok(sent)
    }

    // ---------- Checks ----------

    /// Writes the overall state of the checks into the inbox entries.
    pub(super) fn sync_pr_checks(&mut self, number: impl Into<PrKey>) {
        let key = number.into();
        let Some(checks) = self.review(&key).map(|r| r.checks) else {
            return;
        };
        for item in self.items.iter_mut().filter(|i| {
            i.pr.as_ref()
                .is_some_and(|p| key.matches(&p.repo, p.number))
        }) {
            if let Some(pr) = item.pr.as_mut() {
                pr.checks = checks;
            }
            // Green again: the entry no longer hangs on the checks.
            if checks == Checks::Passed && item.reason == ReasonKind::ChecksFailed {
                item.set_reason(
                    ReasonKind::ReviewRequested,
                    cockpit_i18n::msg!(ReasonChecksPassedAgain),
                );
            }
        }
    }

    /// Restarts a failed check.
    pub fn restart_check(&mut self, number: impl Into<PrKey>, check: &str) -> bool {
        let key = number.into();
        let restarted = self
            .review_mut(&key)
            .is_some_and(|r| r.restart_check(check));
        if restarted {
            self.sync_pr_checks(&key);
        }
        restarted
    }

    // ---------- Merging ----------

    /// What prevents merging pull request `number`. Unknown = empty.
    pub fn merge_blockers(&self, number: impl Into<PrKey>) -> Vec<MergeBlocker> {
        let key = number.into();
        let Some(review) = self.review(&key) else {
            return Vec::new();
        };
        let key = review.key();
        let mut blockers = review.merge_blockers();
        let draft = self.items.iter().any(|i| {
            i.pr.as_ref()
                .is_some_and(|p| key.matches(&p.repo, p.number) && p.draft)
        });
        if draft && review.merged.is_none() {
            blockers.insert(0, MergeBlocker::Draft);
        }
        blockers
    }

    /// Merges the pull request. The inbox entry shows „Gemergt“. The linked ticket
    /// follows via [`Workspace::follow_status`] (if that is switched on); returns its
    /// status change.
    pub fn merge_pr(
        &mut self,
        number: impl Into<PrKey>,
        method: MergeMethod,
    ) -> Result<Vec<StatusChange>, Vec<MergeBlocker>> {
        let key = number.into();
        let blockers = self.merge_blockers(&key);
        if !blockers.is_empty() {
            return Err(blockers);
        }
        let Some(review) = self.review_mut(&key) else {
            return Err(Vec::new());
        };
        review.merged = Some(method);
        let ticket = review.ticket_key.clone();
        let key = review.key();

        for item in self.items.iter_mut().filter(|i| {
            i.pr.as_ref()
                .is_some_and(|p| key.matches(&p.repo, p.number))
        }) {
            if let Some(pr) = item.pr.as_mut() {
                pr.merged = true;
                pr.draft = false;
            }
            item.read = true;
            item.clear_note();
            if item.kind == Kind::PullRequest {
                item.reason_msg = Some(cockpit_i18n::msg!(
                    ReviewMerged,
                    method = crate::action::labels::method(method)
                ));
                item.reason_text.clear();
            }
        }
        // A linked ticket without its own copy of the pull request gets it now.
        if let Some(id) = ticket
            .and_then(|key| self.item_by_key(&key))
            .filter(|i| i.is_ticket() && i.pr.is_none())
            .map(|i| i.id.clone())
        {
            let pr = self.pr_ref_of(&key);
            if let (Some(item), Some(pr)) = (self.item_mut(&id), pr) {
                item.pr = Some(pr);
            }
        }
        Ok(self.follow_status())
    }
}
