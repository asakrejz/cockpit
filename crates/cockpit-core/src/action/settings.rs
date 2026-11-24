use cockpit_i18n::msg;

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
                Ok(vec![Note::coded(
                    Target::Settings,
                    msg!(
                        CoreActionLogNotify,
                        reason = labels::reason(reason),
                        channel = labels::channel(channel),
                        state = labels::on_off(on)
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
                    msg!(
                        CoreActionLogQuietOn,
                        from = QuietHours::format(quiet.from),
                        to = QuietHours::format(quiet.to)
                    )
                } else {
                    msg!(CoreActionLogQuietOff)
                };
                Ok(vec![Note::coded(Target::Settings, text)])
            }
            Action::SetLanguage { language } => {
                if self.language == Some(language) {
                    return Err(ActionError::Unchanged);
                }
                self.language = Some(language);
                Ok(vec![Note::coded(
                    Target::Settings,
                    cockpit_i18n::msg!(CoreSettingsLanguageSet, language = language.name()),
                )])
            }
            Action::SetAgentLimits { limits } => {
                if !limits.is_valid() {
                    return Err(ActionError::InvalidInput);
                }
                let text = msg!(
                    CoreActionLogLimits,
                    summary = labels::limits_summary(&limits)
                );
                let pricing_changed = limits.pricing != self.limits.pricing;
                if !self.set_limits(limits) {
                    return Err(ActionError::Unchanged);
                }
                let mut notes = vec![Note::coded(Target::Settings, text)];
                if pricing_changed {
                    notes.push(Note::coded(
                        Target::Settings,
                        msg!(
                            CoreActionLogPricing,
                            input = format_euro(self.limits.pricing.input_cents_per_mtok),
                            output = format_euro(self.limits.pricing.output_cents_per_mtok)
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
                let lines = labels::rule_lines(&self.rules_for(&repo));
                let text = if lines.is_empty() {
                    msg!(CoreActionLogRulesRemoved, repo = repo)
                } else {
                    msg!(
                        CoreActionLogRulesChanged,
                        repo = repo,
                        lines = cockpit_i18n::ArgValue::joined(lines, " · ")
                    )
                };
                Ok(vec![Note::coded(Target::Settings, text)])
            }
            Action::SyncRemote { remote } => {
                let (merged, changes) = sync::merge(self, &remote, at_ms);
                *self = merged;
                let mut notes: Vec<Note> = changes
                    .into_iter()
                    .map(|c| {
                        let msg = c.msg();
                        Note {
                            actor: Some(Actor::System),
                            target: c.target(),
                            text: msg.de(),
                            msg: Some(msg),
                        }
                    })
                    .collect();
                // The sentence in the result comes from the first change; without a change it stays empty.
                notes.truncate(MAX_LOG);
                Ok(notes)
            }
            _ => unreachable!("settings: dispatched by perform"),
        }
    }
}
