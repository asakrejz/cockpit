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
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("{key} als erledigt markiert"),
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
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Status von {key} auf „{}“ gesetzt", status_label(status)),
                )])
            }
            Action::CreateBranch { item } => {
                let key = self.key_of(&item)?;
                let name = self.create_branch(&item).ok_or(ActionError::BranchExists)?;
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Branch {name} für {key} erstellt"),
                )])
            }
            Action::Reply { item, text } => {
                let key = self.key_of(&item)?;
                if !self.reply(&item, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Antwort an {key} gesendet"),
                )])
            }
            Action::Snooze { item, until_ms } => {
                let key = self.key_of(&item)?;
                if until_ms.is_some_and(|t| t <= at_ms) {
                    return Err(ActionError::InvalidInput);
                }
                self.snooze(&item, until_ms);
                let text = match until_ms {
                    Some(_) => format!("{key} zurückgestellt"),
                    None => format!("{key} zurückgestellt, bis sich etwas ändert"),
                };
                Ok(vec![Note::new(Target::Item(item), text)])
            }
            Action::Unsnooze { item } => {
                let key = self.key_of(&item)?;
                if !self.unsnooze(&item) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("{key} aus der Zurückstellung geholt"),
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
                        Note::new(Target::Item(id), format!("{key} ist zurück in der Inbox"))
                            .by(Actor::System)
                    })
                    .collect())
            }
            Action::SetRule { rule, enabled } => {
                if !self.set_rule(rule, enabled) {
                    return Err(ActionError::Unchanged);
                }
                Ok(vec![Note::general(format!(
                    "Regel „{}“ {}",
                    rule.label(),
                    if enabled {
                        "eingeschaltet"
                    } else {
                        "ausgeschaltet"
                    }
                ))])
            }
            Action::EditTicket {
                item,
                title,
                description,
            } => {
                let key = self.key_of(&item)?;
                let current = self.item(&item).expect("key_of kennt den Eintrag");
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
                let what = match (title_changed, description_changed) {
                    (true, true) => "Titel und Beschreibung",
                    (true, false) => "Titel",
                    _ => "Beschreibung",
                };
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("{what} von {key} geändert"),
                )])
            }
            Action::AssignTicket { item, person } => {
                let key = self.key_of(&item)?;
                let current = self.item(&item).expect("key_of kennt den Eintrag");
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
                    format!("{key} dir zugewiesen")
                } else {
                    format!("{key} an {} zugewiesen", person.name)
                };
                Ok(vec![Note::new(Target::Item(item), text)])
            }
            Action::SetPriority { item, priority } => {
                let key = self.key_of(&item)?;
                match self.item(&item) {
                    Some(i) if !i.is_ticket() => return Err(ActionError::NotATicket),
                    Some(i) if i.priority == priority => return Err(ActionError::Unchanged),
                    _ => {}
                }
                self.set_priority(&item, priority);
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Priorität von {key} auf „{}“ gesetzt", priority.label()),
                )])
            }
            Action::LinkPr { item, pr } => {
                let key = self.key_of(&item)?;
                if !self.item(&item).is_some_and(Item::is_ticket) {
                    return Err(ActionError::NotATicket);
                }
                self.link_pr(&item, pr).map_err(|e| match e {
                    LinkError::Same => ActionError::Unchanged,
                    e => ActionError::Link(e),
                })?;
                let mut notes = vec![Note::new(
                    Target::Item(item),
                    format!("{key} mit #{pr} verknüpft"),
                )];
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::UnlinkPr { item } => {
                let key = self.key_of(&item)?;
                if !self.item(&item).is_some_and(Item::is_ticket) {
                    return Err(ActionError::NotATicket);
                }
                let number = self.unlink_pr(&item).ok_or(ActionError::NothingToDo)?;
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Verknüpfung von {key} mit #{number} gelöst"),
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
                        Note::new(
                            Target::Item(id),
                            format!("{key} automatisch mit #{number} verknüpft"),
                        )
                        .by(Actor::System)
                    })
                    .collect();
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::RequestReview { review } => {
                self.need_open_review(review)?;
                if !self.request_review(review) {
                    return Err(ActionError::Unchanged);
                }
                let mut notes = vec![Note::new(
                    Target::Review(review),
                    format!("Review für #{review} angefragt"),
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
                let mut notes = vec![Note::new(
                    Target::Settings,
                    format!(
                        "Status folgt dem Pull Request: {}",
                        if enabled {
                            "eingeschaltet"
                        } else {
                            "ausgeschaltet"
                        }
                    ),
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
                    Some(p) => format!("Jira-Projekt gewählt: {p}"),
                    None => "Jira-Projekt: alle".to_string(),
                };
                if !self.set_jira_project(project) {
                    return Err(ActionError::Unchanged);
                }
                Ok(vec![Note::new(Target::Settings, text)])
            }
            _ => unreachable!("inbox: dispatched by perform"),
        }
    }
}
