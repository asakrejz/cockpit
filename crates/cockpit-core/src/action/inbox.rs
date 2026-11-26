//! Handlers of the inbox actions: marking, status, snoozing, editing and assigning tickets,
//! linking, and the display settings of the lists.

use cockpit_i18n::msg;

use super::*;

impl Workspace {
    pub(super) fn perform_mark_done(&mut self, item: String) -> Result<Vec<Note>, ActionError> {
        let key = self.key_of(&item)?;
        if self.item(&item).is_some_and(|i| i.read) {
            return Err(ActionError::Unchanged);
        }
        self.mark_done(&item);
        Ok(vec![Note::coded(
            Target::Item(item),
            msg!(CoreActionLogMarkDone, key = key),
        )])
    }

    pub(super) fn perform_set_status(
        &mut self,
        item: String,
        status: TicketStatus,
    ) -> Result<Vec<Note>, ActionError> {
        let key = self.key_of(&item)?;
        let current = self.item(&item).map(|i| (i.is_ticket(), i.status));
        match current {
            Some((false, _)) => return Err(ActionError::NotATicket),
            Some((_, now)) if now == status => return Err(ActionError::Unchanged),
            _ => {}
        }
        self.set_status(&item, status);
        Ok(vec![Note::coded(
            Target::Item(item),
            msg!(
                CoreActionLogStatusSet,
                key = key,
                status = labels::status(status)
            ),
        )])
    }

    pub(super) fn perform_create_branch(&mut self, item: String) -> Result<Vec<Note>, ActionError> {
        let key = self.key_of(&item)?;
        let name = self.create_branch(&item).ok_or(ActionError::BranchExists)?;
        Ok(vec![Note::coded(
            Target::Item(item),
            msg!(CoreActionLogBranchCreated, name = name, key = key),
        )])
    }

    pub(super) fn perform_reply(
        &mut self,
        item: String,
        text: String,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        let key = self.key_of(&item)?;
        if !self.reply(&item, &text, at_ms) {
            return Err(ActionError::InvalidInput);
        }
        Ok(vec![Note::coded(
            Target::Item(item),
            msg!(CoreActionLogReplySent, key = key),
        )])
    }

    pub(super) fn perform_snooze(
        &mut self,
        item: String,
        until_ms: Option<u64>,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        let key = self.key_of(&item)?;
        if until_ms.is_some_and(|t| t <= at_ms) {
            return Err(ActionError::InvalidInput);
        }
        self.snooze(&item, until_ms);
        let text = match until_ms {
            Some(_) => msg!(CoreActionLogSnoozed, key = key),
            None => msg!(CoreActionLogSnoozedUntilChange, key = key),
        };
        Ok(vec![Note::coded(Target::Item(item), text)])
    }

    pub(super) fn perform_unsnooze(&mut self, item: String) -> Result<Vec<Note>, ActionError> {
        let key = self.key_of(&item)?;
        if !self.unsnooze(&item) {
            return Err(ActionError::NothingToDo);
        }
        Ok(vec![Note::coded(
            Target::Item(item),
            msg!(CoreActionLogUnsnoozed, key = key),
        )])
    }

    pub(super) fn perform_wake_snoozed(&mut self, at_ms: u64) -> Result<Vec<Note>, ActionError> {
        let back = self.wake(at_ms);
        if back.is_empty() {
            return Err(ActionError::NothingToDo);
        }
        Ok(back
            .into_iter()
            .map(|id| {
                let key = self.item(&id).map_or(id.clone(), |i| i.key.clone());
                Note::coded(Target::Item(id), msg!(CoreActionLogWoke, key = key)).by(Actor::System)
            })
            .collect())
    }

    pub(super) fn perform_set_rule(
        &mut self,
        rule: Rule,
        enabled: bool,
    ) -> Result<Vec<Note>, ActionError> {
        if !self.set_rule(rule, enabled) {
            return Err(ActionError::Unchanged);
        }
        Ok(vec![Note::coded_general(msg!(
            CoreActionLogRule,
            rule = labels::rule(rule),
            state = labels::on_off(enabled)
        ))])
    }

    pub(super) fn perform_edit_ticket(
        &mut self,
        item: String,
        title: String,
        description: String,
    ) -> Result<Vec<Note>, ActionError> {
        let key = self.key_of(&item)?;
        let current = self.item(&item).expect("key_of knows the entry");
        if !current.is_ticket() {
            return Err(ActionError::NotATicket);
        }
        if title.trim().is_empty() {
            return Err(ActionError::InvalidInput);
        }
        let title_changed = current.title != title.trim();
        let description_changed = current.description != description.trim();
        if !title_changed && !description_changed {
            return Err(ActionError::Unchanged);
        }
        self.edit_ticket(&item, &title, &description);
        let text = match (title_changed, description_changed) {
            (true, true) => msg!(CoreActionLogEditedBoth, key = key),
            (true, false) => msg!(CoreActionLogEditedTitle, key = key),
            _ => msg!(CoreActionLogEditedDescription, key = key),
        };
        Ok(vec![Note::coded(Target::Item(item), text)])
    }

    pub(super) fn perform_assign_ticket(
        &mut self,
        item: String,
        person: Person,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        let key = self.key_of(&item)?;
        let current = self.item(&item).expect("key_of knows the entry");
        if !current.is_ticket() {
            return Err(ActionError::NotATicket);
        }
        // Anyone with a name can be picked – people found by the search in Jira are not in
        // the synced tickets yet. The agent gets tickets through its run.
        if person.is_agent() || !(person.me || person.nobody || !person.name.trim().is_empty()) {
            return Err(ActionError::UnknownPerson(person.name));
        }
        if current.owner == person {
            return Err(ActionError::Unchanged);
        }
        self.assign(&item, &person, at_ms);
        let text = if person == Person::me() {
            msg!(CoreActionLogAssignedMe, key = key)
        } else {
            msg!(CoreActionLogAssigned, key = key, name = person.name_msg())
        };
        Ok(vec![Note::coded(Target::Item(item), text)])
    }

    pub(super) fn perform_set_priority(
        &mut self,
        item: String,
        priority: Priority,
    ) -> Result<Vec<Note>, ActionError> {
        let key = self.key_of(&item)?;
        match self.item(&item) {
            Some(i) if !i.is_ticket() => return Err(ActionError::NotATicket),
            Some(i) if i.priority == priority => return Err(ActionError::Unchanged),
            _ => {}
        }
        self.set_priority(&item, priority);
        Ok(vec![Note::coded(
            Target::Item(item),
            msg!(
                CoreActionLogPrioritySet,
                key = key,
                priority = labels::priority(priority)
            ),
        )])
    }

    pub(super) fn perform_auto_link_prs(&mut self) -> Result<Vec<Note>, ActionError> {
        let linked = self.auto_link();
        if linked.is_empty() {
            return Err(ActionError::NothingToDo);
        }
        let mut notes: Vec<Note> = linked
            .into_iter()
            .map(|(id, key, number)| {
                Note::coded(
                    Target::Item(id),
                    msg!(CoreActionLogAutoLinked, key = key, pr = number.to_string()),
                )
                .by(Actor::System)
            })
            .collect();
        notes.extend(status_notes(self.follow_status()));
        Ok(notes)
    }

    pub(super) fn perform_sync_ticket_status(&mut self) -> Result<Vec<Note>, ActionError> {
        // An error changes nothing: first check whether a pull request has moved.
        if !self.status_pending() {
            return Err(ActionError::NothingToDo);
        }
        // If the status already fits, the state is only noted – without an entry.
        Ok(status_notes(self.follow_status()))
    }

    pub(super) fn perform_set_status_follows_pr(
        &mut self,
        enabled: bool,
    ) -> Result<Vec<Note>, ActionError> {
        if !self.set_status_follows_pr(enabled) {
            return Err(ActionError::Unchanged);
        }
        let mut notes = vec![Note::coded(
            Target::Settings,
            msg!(CoreActionLogStatusFollows, state = labels::on_off(enabled)),
        )];
        // Switched on again: whatever has moved in the meantime is brought in line.
        notes.extend(status_notes(self.follow_status()));
        Ok(notes)
    }

    pub(super) fn perform_select_jira_project(
        &mut self,
        project: Option<String>,
    ) -> Result<Vec<Note>, ActionError> {
        let project = project
            .map(|p| p.trim().to_uppercase())
            .filter(|p| !p.is_empty());
        if project
            .as_deref()
            .is_some_and(|p| !crate::is_project_key(p))
        {
            return Err(ActionError::InvalidInput);
        }
        let text = match &project {
            Some(p) => msg!(CoreActionLogJiraProject, project = p),
            None => msg!(CoreActionLogJiraProjectAll),
        };
        if !self.set_jira_project(project) {
            return Err(ActionError::Unchanged);
        }
        Ok(vec![Note::coded(Target::Settings, text)])
    }

    pub(super) fn perform_set_team_choice(
        &mut self,
        choice: crate::team_filter::TeamChoice,
    ) -> Result<Vec<Note>, ActionError> {
        let text = if choice.is_active() {
            msg!(
                CoreActionLogTeamChosen,
                teams = choice
                    .teams
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            msg!(CoreActionLogTeamAll)
        };
        if !self.set_team_choice(choice) {
            return Err(ActionError::Unchanged);
        }
        Ok(vec![Note::coded(Target::Settings, text)])
    }

    pub(super) fn perform_set_people_source(
        &mut self,
        source: crate::people_link::PeopleSource,
    ) -> Result<Vec<Note>, ActionError> {
        if !self.set_people_source(source) {
            return Err(ActionError::Unchanged);
        }
        Ok(vec![Note::coded(
            Target::Settings,
            match source {
                crate::PeopleSource::All => msg!(CoreActionLogPeopleAll),
                crate::PeopleSource::Jira => msg!(CoreActionLogPeopleJira),
                crate::PeopleSource::GitHub => msg!(CoreActionLogPeopleGithub),
            },
        )])
    }

    pub(super) fn perform_link_people(
        &mut self,
        jira: String,
        github: String,
    ) -> Result<Vec<Note>, ActionError> {
        if !self.link_people(&jira, &github) {
            let valid = !jira.trim().is_empty() && !github.trim().is_empty();
            return Err(if valid && self.link_of_jira(jira.trim()).is_some() {
                ActionError::Unchanged
            } else {
                ActionError::InvalidInput
            });
        }
        Ok(vec![Note::coded(
            Target::Settings,
            msg!(
                CoreActionLogPeopleLinked,
                jira = jira.trim(),
                github = github.trim()
            ),
        )])
    }

    pub(super) fn perform_unlink_people(&mut self, jira: String) -> Result<Vec<Note>, ActionError> {
        if !self.unlink_people(&jira) {
            return Err(ActionError::Unchanged);
        }
        Ok(vec![Note::coded(
            Target::Settings,
            msg!(CoreActionLogPeopleUnlinked, jira = &jira),
        )])
    }

    pub(super) fn perform_set_inbox_filter(
        &mut self,
        filter: crate::InboxFilter,
    ) -> Result<Vec<Note>, ActionError> {
        let text = if filter.is_default() {
            msg!(CoreActionLogFilterReset)
        } else {
            msg!(CoreActionLogFilterApplied)
        };
        if !self.set_inbox_filter(filter) {
            return Err(ActionError::Unchanged);
        }
        Ok(vec![Note::coded(Target::Settings, text)])
    }

    pub(super) fn perform_set_review_filter(
        &mut self,
        filter: crate::review_filter::ReviewFilter,
    ) -> Result<Vec<Note>, ActionError> {
        let text = if filter.is_default() {
            msg!(CoreActionLogFilterReset)
        } else {
            msg!(CoreActionLogFilterApplied)
        };
        if !self.set_review_filter(filter) {
            return Err(ActionError::Unchanged);
        }
        Ok(vec![Note::coded(Target::Settings, text)])
    }
}
