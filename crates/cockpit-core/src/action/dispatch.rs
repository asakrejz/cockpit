//! Applying an action: the actor check, the one place that sends every variant to its handler,
//! and the log.
//!
//! `perform` has exactly one arm per [`Action`] variant and no wildcard arm: a new variant does
//! not compile until it is routed here, and every variant has exactly one handler (a method
//! `perform_<variant>` in the file of its topic).

use super::*;

impl Workspace {
    /// Applies an action and writes it into the log.
    ///
    /// `actor` is who triggered it, `at_ms` the point in time (milliseconds since 1970;
    /// the time comes from outside so that the logic stays testable). On an error
    /// workspace and log stay unchanged.
    pub fn apply(
        &mut self,
        action: Action,
        actor: Actor,
        at_ms: u64,
    ) -> Result<Applied, ActionError> {
        // The actor check runs before anything else.
        if let Some(required) = action.required_actor()
            && actor != required
        {
            return Err(match required {
                // Merging has a message of its own.
                Actor::User if matches!(action, Action::MergePr { .. }) => {
                    ActionError::MergeNotAllowed(actor)
                }
                Actor::User => ActionError::UserOnly(actor),
                Actor::System => ActionError::SystemOnly,
                Actor::Agent => ActionError::AgentOnly,
            });
        }
        let notes = self.perform(action, at_ms)?;
        let logged = notes.len();
        for note in notes {
            self.push_log(at_ms, note.actor.unwrap_or(actor), note.target, note.msg);
        }
        Ok(Applied { logged })
    }

    fn push_log(
        &mut self,
        at_ms: u64,
        actor: Actor,
        target: Option<Target>,
        msg: cockpit_i18n::Msg,
    ) {
        let id = self.log.last().map_or(1, |e| e.id + 1);
        self.log.push(LogEntry {
            id,
            at_ms,
            actor,
            target,
            text: String::new(),
            msg: Some(msg),
        });
        if self.log.len() > MAX_LOG {
            let excess = self.log.len() - MAX_LOG;
            self.log.drain(..excess);
        }
    }

    /// The log, newest entries first. With `actor` only that actor's entries.
    pub fn log_entries(&self, actor: Option<Actor>) -> Vec<&LogEntry> {
        self.log
            .iter()
            .rev()
            .filter(|e| actor.is_none_or(|a| e.actor == a))
            .collect()
    }

    pub(super) fn key_of(&self, id: &str) -> Result<String, ActionError> {
        self.item(id)
            .map(|i| i.key.clone())
            .ok_or_else(|| ActionError::UnknownItem(id.to_string()))
    }

    pub(super) fn need_review(&self, number: &PrKey) -> Result<(), ActionError> {
        self.review(number)
            .map(|_| ())
            .ok_or_else(|| ActionError::UnknownReview(number.clone()))
    }

    /// Like `need_review`, but a merged pull request can no longer be changed.
    pub(super) fn need_open_review(&self, number: &PrKey) -> Result<(), ActionError> {
        match self.review(number) {
            None => Err(ActionError::UnknownReview(number.clone())),
            Some(r) if r.merged.is_some() => Err(ActionError::Merged(r.key())),
            Some(_) => Ok(()),
        }
    }

    /// Executes the action and returns the log entries for it.
    fn perform(&mut self, action: Action, at_ms: u64) -> Result<Vec<Note>, ActionError> {
        match action {
            // inbox.rs: inbox, tickets, display settings
            Action::MarkDone { item } => self.perform_mark_done(item),
            Action::SetStatus { item, status } => self.perform_set_status(item, status),
            Action::CreateBranch { item } => self.perform_create_branch(item),
            Action::Reply { item, text } => self.perform_reply(item, text, at_ms),
            Action::Snooze { item, until_ms } => self.perform_snooze(item, until_ms, at_ms),
            Action::Unsnooze { item } => self.perform_unsnooze(item),
            Action::WakeSnoozed => self.perform_wake_snoozed(at_ms),
            Action::SetRule { rule, enabled } => self.perform_set_rule(rule, enabled),
            Action::EditTicket {
                item,
                title,
                description,
            } => self.perform_edit_ticket(item, title, description),
            Action::AssignTicket { item, person } => {
                self.perform_assign_ticket(item, person, at_ms)
            }
            Action::SetPriority { item, priority } => self.perform_set_priority(item, priority),
            Action::AutoLinkPrs => self.perform_auto_link_prs(),
            Action::SyncTicketStatus => self.perform_sync_ticket_status(),
            Action::SetStatusFollowsPr { enabled } => self.perform_set_status_follows_pr(enabled),
            Action::SelectJiraProject { project } => self.perform_select_jira_project(project),
            Action::SetTeamChoice { choice } => self.perform_set_team_choice(choice),
            Action::SetPeopleSource { source } => self.perform_set_people_source(source),
            Action::LinkPeople { jira, github } => self.perform_link_people(jira, github),
            Action::UnlinkPeople { jira } => self.perform_unlink_people(jira),
            Action::SetInboxFilter { filter } => self.perform_set_inbox_filter(filter),
            Action::SetReviewFilter { filter } => self.perform_set_review_filter(filter),
            // projects.rs: tickets of GitHub projects
            Action::SetLabel { item, label, on } => self.perform_set_label(item, label, on),
            Action::SetIssueClosed { item, closed } => self.perform_set_issue_closed(item, closed),
            Action::SetProjectField {
                item,
                field_id,
                value,
            } => self.perform_set_project_field(item, field_id, value),
            Action::CreateIssue {
                project,
                repo,
                title,
                body,
                status,
                created,
            } => self.perform_create_issue(project, repo, title, body, status, created, at_ms),
            Action::ConvertDraft {
                item,
                repo,
                created,
            } => self.perform_convert_draft(item, repo, created),
            // review.rs: pull request reviews
            Action::Decide {
                review,
                suggestion,
                decision,
            } => self.perform_decide(review, suggestion, decision),
            Action::EditSuggestion {
                review,
                suggestion,
                text,
            } => self.perform_edit_suggestion(review, suggestion, text),
            Action::AddLineComment {
                review,
                file,
                line,
                text,
                suggestion,
            } => self.perform_add_line_comment(review, file, line, text, suggestion),
            Action::EditLineComment {
                review,
                comment,
                text,
                suggestion,
            } => self.perform_edit_line_comment(review, comment, text, suggestion),
            Action::DeleteLineComment { review, comment } => {
                self.perform_delete_line_comment(review, comment)
            }
            Action::SetVerdict { review, verdict } => self.perform_set_verdict(review, verdict),
            Action::SendReview { review, comment } => {
                self.perform_send_review(review, comment, at_ms)
            }
            Action::RestartCheck { review, check } => self.perform_restart_check(review, check),
            Action::MergePr { review, method } => self.perform_merge_pr(review, method),
            // agent.rs: agent runs and the jobs around them
            Action::StartAgent { ticket, options } => self.perform_start_agent(ticket, options),
            Action::CancelAgent { ticket } => self.perform_cancel_agent(ticket),
            Action::ApprovePlan { ticket } => self.perform_approve_plan(ticket),
            Action::RejectPlan { ticket } => self.perform_reject_plan(ticket),
            Action::RevisePlan { ticket, note } => self.perform_revise_plan(ticket, note),
            Action::AnswerAgent { run, text } => self.perform_answer_agent(run, text),
            Action::SteerAgent { run, text } => self.perform_steer_agent(run, text),
            Action::PauseAgent { run } => self.perform_pause_agent(run),
            Action::ResumeAgent { run } => self.perform_resume_agent(run),
            Action::ReportAgent { run, report } => self.perform_report_agent(run, report, at_ms),
            Action::CloseAgentRun { run, closure } => self.perform_close_agent_run(run, closure),
            Action::RemoveAgentRun { run } => self.perform_remove_agent_run(run),
            Action::RemoveFinishedRuns => self.perform_remove_finished_runs(),
            Action::AddressReviewThreads { review, options } => {
                self.perform_address_review_threads(review, options)
            }
            Action::StartPrReview {
                review,
                language,
                repo_language,
            } => self.perform_start_pr_review(review, language, repo_language),
            Action::FixFailedChecks { review, options } => {
                self.perform_fix_failed_checks(review, options)
            }
            Action::CreatePullRequest {
                run,
                title,
                body,
                pull,
                repo,
            } => self.perform_create_pull_request(run, title, body, pull, repo),
            Action::ResumeStopped { run, extra } => self.perform_resume_stopped(run, extra, at_ms),
            // settings.rs: notifications, limits, rules, language, sync
            Action::SetNotifyChannel {
                reason,
                channel,
                on,
            } => self.perform_set_notify_channel(reason, channel, on),
            Action::SetQuietHours { quiet } => self.perform_set_quiet_hours(quiet),
            Action::SetLanguage { language } => self.perform_set_language(language),
            Action::SetAgentLimits { limits } => self.perform_set_agent_limits(limits),
            Action::SetRepoRules { repo, rules } => self.perform_set_repo_rules(repo, rules),
            Action::SyncRemote { remote } => self.perform_sync_remote(remote, at_ms),
        }
    }
}
