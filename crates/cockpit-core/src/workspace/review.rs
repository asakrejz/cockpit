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

    /// Lets the agent look through a pull request. Returns the number of suggestions.
    pub fn request_agent_review(&mut self, number: impl Into<PrKey>) -> usize {
        let key = number.into();
        let count = self.review_mut(&key).map_or(0, Review::run_agent_review);
        if let Some(item) = self.pr_item_mut(&key) {
            item.agent_note = Some(match count {
                0 => "Der Agent hat nichts zu beanstanden".to_string(),
                1 => "1 Kommentar vom Agenten wartet auf deine Freigabe".to_string(),
                n => format!("{n} Kommentare vom Agenten warten auf deine Freigabe"),
            });
        }
        count
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
    ) -> Result<usize, SendError> {
        let key = number.into();
        let review = self.review_mut(&key).ok_or(SendError::UnknownReview)?;
        let verdict = review.verdict;
        let sent = review.send()?.len();

        let mut text = format!(
            "Review gesendet: {} · {sent} {}",
            verdict.label(),
            if sent == 1 { "Kommentar" } else { "Kommentare" }
        );
        if !own_comment.trim().is_empty() {
            text.push('\n');
            text.push_str(own_comment.trim());
        }
        if let Some(item) = self.pr_item_mut(&key) {
            item.comments.push(Comment {
                own: true,
                ..Comment::new(Person::me(), "gerade eben", &text)
            });
            item.agent_note = None;
            item.read = true;
            // The review is submitted; the request is answered.
            if item.reason == ReasonKind::ReviewRequested {
                item.reason = ReasonKind::Comment;
            }
        }
        Ok(sent)
    }

    // ---------- Checks ----------

    /// Checks that are still running (across all pull requests). Pull requests from GitHub do not
    /// count: their checks run there, the sync fetches the result.
    pub fn running_checks(&self) -> usize {
        self.reviews
            .iter()
            .filter(|r| self.sync.pr(&r.key()).is_none())
            .map(Review::running_checks)
            .sum()
    }

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
                item.reason = ReasonKind::ReviewRequested;
                item.reason_text = "Checks wieder bestanden".into();
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

    /// The tick of the checks: every running one advances one step. Returns
    /// `(pull request, check, result)` for each one that finished in the process.
    pub fn advance_checks(&mut self) -> Vec<(PrKey, String, Checks)> {
        let mut finished = Vec::new();
        let linked: Vec<PrKey> = self.sync.prs.iter().map(crate::sync::PrLink::key).collect();
        for review in self
            .reviews
            .iter_mut()
            .filter(|r| !linked.iter().any(|l| l.same(&r.key())))
        {
            for name in review.advance_checks() {
                let status = review
                    .check_runs
                    .iter()
                    .find(|c| c.name == name)
                    .map_or(Checks::Failed, |c| c.status);
                finished.push((review.key(), name, status));
            }
        }
        let keys: Vec<PrKey> = finished.iter().map(|(k, ..)| k.clone()).collect();
        for k in keys {
            self.sync_pr_checks(k);
        }
        finished
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
            item.agent_note = None;
            if item.kind == Kind::PullRequest {
                item.reason_text = format!("Gemergt ({})", method.label());
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

    /// Requests a review: the pull request counts as „Review angefragt“. `false` if it already is.
    pub fn request_review(&mut self, number: impl Into<PrKey>) -> bool {
        let key = number.into();
        let mut changed = false;
        for pr in self
            .items
            .iter_mut()
            .filter_map(|i| i.pr.as_mut())
            .filter(|p| key.matches(&p.repo, p.number) && !p.review_requested)
        {
            pr.review_requested = true;
            changed = true;
        }
        changed
    }
}
