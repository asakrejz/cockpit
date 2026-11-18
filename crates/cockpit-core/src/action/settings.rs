use super::*;

impl Workspace {
    pub(super) fn perform_settings(
        &mut self,
        action: Action,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        match action {
            Action::SetNotifyChannel {
                reason,
                channel,
                on,
            } => {
                if !self.notifications.set(reason, channel, on) {
                    return Err(ActionError::Unchanged);
                }
                Ok(vec![Note::new(
                    Target::Settings,
                    format!(
                        "Benachrichtigung „{}“ über {} {}",
                        reason.label(),
                        channel.label(),
                        if on { "eingeschaltet" } else { "ausgeschaltet" }
                    ),
                )])
            }
            Action::SetQuietHours { quiet } => {
                if !quiet.is_valid() {
                    return Err(ActionError::InvalidInput);
                }
                if !self.notifications.set_quiet(quiet) {
                    return Err(ActionError::Unchanged);
                }
                let text = if quiet.enabled {
                    format!(
                        "Ruhezeiten von {} bis {} Uhr eingeschaltet",
                        QuietHours::format(quiet.from),
                        QuietHours::format(quiet.to)
                    )
                } else {
                    "Ruhezeiten ausgeschaltet".to_string()
                };
                Ok(vec![Note::new(Target::Settings, text)])
            }
            Action::SetLanguage { language } => {
                if self.language == Some(language) {
                    return Err(ActionError::Unchanged);
                }
                self.language = Some(language);
                Ok(vec![Note::new(
                    Target::Settings,
                    format!("Sprache der Oberfläche: {}", language.name()),
                )])
            }
            Action::SetAgentLimits { limits } => {
                if !limits.is_valid() {
                    return Err(ActionError::InvalidInput);
                }
                let text = format!("Grenzen für Agenten geändert: {}", limits.summary());
                let pricing_changed = limits.pricing != self.limits.pricing;
                if !self.set_limits(limits) {
                    return Err(ActionError::Unchanged);
                }
                let mut notes = vec![Note::new(Target::Settings, text)];
                if pricing_changed {
                    notes.push(Note::new(
                        Target::Settings,
                        format!(
                            "Preise geändert: {} je Million Token ein, {} aus",
                            format_euro(self.limits.pricing.input_cents_per_mtok),
                            format_euro(self.limits.pricing.output_cents_per_mtok)
                        ),
                    ));
                }
                Ok(notes)
            }
            Action::SetRepoRules { repo, rules } => {
                if !crate::model::repo_name_valid(&repo) {
                    return Err(ActionError::UnknownRepo(repo));
                }
                if !self.set_repo_rules(&repo, rules) {
                    return Err(ActionError::Unchanged);
                }
                let lines = self.rules_for(&repo).lines();
                let text = if lines.is_empty() {
                    format!("Regeln für {repo} entfernt")
                } else {
                    format!("Regeln für {repo} geändert: {}", lines.join(" · "))
                };
                Ok(vec![Note::new(Target::Settings, text)])
            }
            Action::SyncRemote { remote } => {
                let (merged, changes) = sync::merge(self, &remote, at_ms);
                *self = merged;
                let mut notes: Vec<Note> = changes
                    .into_iter()
                    .map(|c| Note {
                        actor: Some(Actor::System),
                        target: c.target(),
                        text: c.text(),
                    })
                    .collect();
                // The sentence in the result comes from the first change; without a change it stays empty.
                notes.truncate(MAX_LOG);
                Ok(notes)
            }
            Action::ResetDemo => {
                // The settings do not belong to the sample data and stay.
                let notifications = std::mem::take(&mut self.notifications);
                let sources = std::mem::take(&mut self.sync.sources);
                let pr_status_off = self.pr_status_off;
                let limits = self.limits.clone();
                let repo_rules = std::mem::take(&mut self.repo_rules);
                let jira_project = self.jira_project.take();
                let inbox_filter = std::mem::take(&mut self.inbox_filter);
                let language = self.language;
                *self = demo::workspace();
                self.jira_project = jira_project;
                self.inbox_filter = inbox_filter;
                self.language = language;
                self.notifications = notifications;
                self.pr_status_off = pr_status_off;
                self.limits = limits;
                self.repo_rules = repo_rules;
                self.sync.sources = sources;
                Ok(vec![
                    Note::new(
                        Target::Item(self.items.first().map(|i| i.id.clone()).unwrap_or_default()),
                        "Demo-Daten zurückgesetzt",
                    )
                    .by(Actor::System),
                ])
            }
            _ => unreachable!("settings: dispatched by perform"),
        }
    }
}
