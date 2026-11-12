use super::*;

impl Workspace {
    // ---------- Review ----------

    pub fn decide(&mut self, number: u32, suggestion: &str, decision: Option<Decision>) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.decide(suggestion, decision))
    }

    pub fn edit_suggestion(&mut self, number: u32, suggestion: &str, text: &str) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.edit(suggestion, text))
    }

    /// Own comment on a diff line. Returns the ID of the comment.
    pub fn add_line_comment(
        &mut self,
        number: u32,
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
        number: u32,
        comment: &str,
        text: &str,
        suggestion: Option<&str>,
    ) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.edit_line_comment(comment, text, suggestion))
    }

    pub fn delete_line_comment(&mut self, number: u32, comment: &str) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.delete_line_comment(comment))
    }

    pub fn set_verdict(&mut self, number: u32, verdict: Verdict) {
        if let Some(r) = self.review_mut(number) {
            r.set_verdict(verdict);
        }
    }

    /// Lets the agent look through a pull request. Returns the number of suggestions.
    pub fn request_agent_review(&mut self, number: u32) -> usize {
        let count = self.review_mut(number).map_or(0, Review::run_agent_review);
        if let Some(item) = self.pr_item_mut(number) {
            item.agent_note = Some(match count {
                0 => "Der Agent hat nichts zu beanstanden".to_string(),
                1 => "1 Kommentar vom Agenten wartet auf deine Freigabe".to_string(),
                n => format!("{n} Kommentare vom Agenten warten auf deine Freigabe"),
            });
        }
        count
    }

    /// The entry that stands for this pull request in the inbox.
    pub(super) fn pr_item_mut(&mut self, number: u32) -> Option<&mut Item> {
        self.items
            .iter_mut()
            .find(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
    }

    /// Sends the review. The inbox entry gets a summary as
    /// an own comment and counts as done. Returns the number of comments.
    pub fn send_review(&mut self, number: u32, own_comment: &str) -> Result<usize, SendError> {
        let review = self.review_mut(number).ok_or(SendError::UnknownReview)?;
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
        if let Some(item) = self.pr_item_mut(number) {
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
            .filter(|r| self.sync.pr(r.number).is_none())
            .map(Review::running_checks)
            .sum()
    }

    /// Writes the overall state of the checks into the inbox entries.
    pub(super) fn sync_pr_checks(&mut self, number: u32) {
        let Some(checks) = self.review(number).map(|r| r.checks) else {
            return;
        };
        for item in self
            .items
            .iter_mut()
            .filter(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
        {
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
    pub fn restart_check(&mut self, number: u32, check: &str) -> bool {
        let restarted = self
            .review_mut(number)
            .is_some_and(|r| r.restart_check(check));
        if restarted {
            self.sync_pr_checks(number);
        }
        restarted
    }

    /// The tick of the checks: every running one advances one step. Returns
    /// `(pull request, check, result)` for each one that finished in the process.
    pub fn advance_checks(&mut self) -> Vec<(u32, String, Checks)> {
        let mut finished = Vec::new();
        let linked: Vec<u32> = self.sync.prs.iter().map(|p| p.number).collect();
        for review in self
            .reviews
            .iter_mut()
            .filter(|r| !linked.contains(&r.number))
        {
            for name in review.advance_checks() {
                let status = review
                    .check_runs
                    .iter()
                    .find(|c| c.name == name)
                    .map_or(Checks::Failed, |c| c.status);
                finished.push((review.number, name, status));
            }
        }
        let numbers: Vec<u32> = finished.iter().map(|(n, ..)| *n).collect();
        for n in numbers {
            self.sync_pr_checks(n);
        }
        finished
    }

    // ---------- Merging ----------

    /// What prevents merging pull request `number`. Unknown = empty.
    pub fn merge_blockers(&self, number: u32) -> Vec<MergeBlocker> {
        let Some(review) = self.review(number) else {
            return Vec::new();
        };
        let mut blockers = review.merge_blockers();
        let draft = self
            .items
            .iter()
            .any(|i| i.pr.as_ref().is_some_and(|p| p.number == number && p.draft));
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
        number: u32,
        method: MergeMethod,
    ) -> Result<Vec<StatusChange>, Vec<MergeBlocker>> {
        let blockers = self.merge_blockers(number);
        if !blockers.is_empty() {
            return Err(blockers);
        }
        let Some(review) = self.review_mut(number) else {
            return Err(Vec::new());
        };
        review.merged = Some(method);
        let ticket = review.ticket_key.clone();

        for item in self
            .items
            .iter_mut()
            .filter(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
        {
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
            let pr = self.pr_ref_of(number);
            if let (Some(item), Some(pr)) = (self.item_mut(&id), pr) {
                item.pr = Some(pr);
            }
        }
        Ok(self.follow_status())
    }

    /// Requests a review: the pull request counts as „Review angefragt“. `false` if it already is.
    pub fn request_review(&mut self, number: u32) -> bool {
        let mut changed = false;
        for pr in self
            .items
            .iter_mut()
            .filter_map(|i| i.pr.as_mut())
            .filter(|p| p.number == number && !p.review_requested)
        {
            pr.review_requested = true;
            changed = true;
        }
        changed
    }
}
