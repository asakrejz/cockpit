use cockpit_i18n::msg;

use super::*;

impl Workspace {
    pub(super) fn perform_inbox(
        &mut self,
        action: Action,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        match action {
            Action::MarkDone { item } => {
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
            Action::SetStatus { item, status } => {
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
            Action::CreateBranch { item } => {
                let key = self.key_of(&item)?;
                let name = self.create_branch(&item).ok_or(ActionError::BranchExists)?;
                Ok(vec![Note::coded(
                    Target::Item(item),
                    msg!(CoreActionLogBranchCreated, name = name, key = key),
                )])
            }
            Action::Reply { item, text } => {
                let key = self.key_of(&item)?;
                if !self.reply(&item, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::coded(
                    Target::Item(item),
                    msg!(CoreActionLogReplySent, key = key),
                )])
            }
            Action::Snooze { item, until_ms } => {
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
            Action::Unsnooze { item } => {
                let key = self.key_of(&item)?;
                if !self.unsnooze(&item) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::coded(
                    Target::Item(item),
                    msg!(CoreActionLogUnsnoozed, key = key),
                )])
            }
            Action::WakeSnoozed => {
                let back = self.wake(at_ms);
                if back.is_empty() {
                    return Err(ActionError::NothingToDo);
                }
                Ok(back
                    .into_iter()
                    .map(|id| {
                        let key = self.item(&id).map_or(id.clone(), |i| i.key.clone());
                        Note::coded(Target::Item(id), msg!(CoreActionLogWoke, key = key))
                            .by(Actor::System)
                    })
                    .collect())
            }
            Action::SetRule { rule, enabled } => {
                if !self.set_rule(rule, enabled) {
                    return Err(ActionError::Unchanged);
                }
                Ok(vec![Note::coded_general(msg!(
                    CoreActionLogRule,
                    rule = labels::rule(rule),
                    state = labels::on_off(enabled)
                ))])
            }
            Action::EditTicket {
                item,
                title,
                description,
            } => {
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
            Action::AssignTicket { item, person } => {
                let key = self.key_of(&item)?;
                let current = self.item(&item).expect("key_of knows the entry");
                if !current.is_ticket() {
                    return Err(ActionError::NotATicket);
                }
                if !self.people().contains(&person) {
                    return Err(ActionError::UnknownPerson(person.name));
                }
                if current.owner == person {
                    return Err(ActionError::Unchanged);
                }
                self.assign(&item, &person);
                let text = if person == Person::me() {
                    msg!(CoreActionLogAssignedMe, key = key)
                } else {
                    msg!(CoreActionLogAssigned, key = key, name = &person.name)
                };
                Ok(vec![Note::coded(Target::Item(item), text)])
            }
            Action::SetPriority { item, priority } => {
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
            Action::AutoLinkPrs => {
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
            Action::RequestReview { review } => {
                self.need_open_review(&review)?;
                if !self.request_review(&review) {
                    return Err(ActionError::Unchanged);
                }
                let mut notes = vec![Note::coded(
                    Target::Review(review.clone()),
                    msg!(CoreActionLogReviewRequested, review = review.to_string()),
                )];
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::SyncTicketStatus => {
                // An error changes nothing: first check whether a pull request has moved.
                if !self.status_pending() {
                    return Err(ActionError::NothingToDo);
                }
                // If the status already fits, the state is only noted – without an entry.
                Ok(status_notes(self.follow_status()))
            }
            Action::SetStatusFollowsPr { enabled } => {
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
            Action::SelectJiraProject { project } => {
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
            Action::SetTeamChoice { choice } => {
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
            Action::SetInboxFilter { filter } => {
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
            _ => unreachable!("inbox: dispatched by perform"),
        }
    }
}
