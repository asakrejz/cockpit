use super::*;

impl AgentRun {
    /// The log of a real run: what the agent and you said, plus the consumption.
    pub(super) fn real_log(&self) -> String {
        let mut lines: Vec<String> = self
            .messages
            .iter()
            .map(|m| {
                let who = match m.from {
                    Speaker::Agent => "Agent",
                    Speaker::User => "Du",
                };
                format!("› {who}: {}", m.text)
            })
            .collect();
        let usage = self.usage();
        if !usage.is_empty() {
            lines.push(format!("› Verbrauch: {}", usage.line()));
        }
        if lines.is_empty() {
            return "Noch keine Aktivität.".into();
        }
        lines.join("\n")
    }

    /// Log lines of a step of „implement review comments“.
    pub(super) fn address_log(
        &self,
        lines: &mut Vec<String>,
        step: usize,
        review: &PrKey,
        comments: usize,
    ) {
        let o = &self.options;
        let hint = o.hint.trim();
        match step {
            0 => {
                lines.push(format!(
                    "› Lese {} aus Review {review}",
                    plural_comments(comments)
                ));
                if !hint.is_empty() {
                    lines.push(format!("› Hinweis: {hint}"));
                }
                if let Some(plan) = self.plan() {
                    lines.push(format!(
                        "› Plan erstellt: {} Schritte, {} Dateien",
                        plan.steps.len(),
                        plan.files.len()
                    ));
                    for note in &self.plan_notes {
                        lines.push(format!("› Neuer Plan angefordert: {note}"));
                    }
                    lines.push(if self.is_awaiting() {
                        "› Warte auf Freigabe".into()
                    } else {
                        "› Plan freigegeben".into()
                    });
                }
            }
            1 => lines.push(format!("› git switch {}", self.branch())),
            2 => lines.push(format!("› Bearbeite {}", self.files.join(", "))),
            3 if o.run_tests => lines.push("› Starte Lint und Tests … bestanden".into()),
            3 => lines.push("› Tests übersprungen".into()),
            _ => lines.push(format!(
                "› git push origin {} (kein neuer Pull Request)",
                self.branch()
            )),
        }
    }

    /// Log lines of a step of „investigate check“.
    pub(super) fn diagnose_log(
        &self,
        lines: &mut Vec<String>,
        step: usize,
        review: &PrKey,
        check: &str,
    ) {
        let code = self.diagnosis.as_ref().is_none_or(Diagnosis::changes_code);
        match step {
            0 => lines.push(format!("› Lese das Log von „{check}“ in {review}")),
            1 => lines.push("› Suche Fehlermeldung, Datei und Zeile".into()),
            2 => lines.push(match &self.diagnosis {
                Some(d) => format!("› Diagnose: {}", d.cause),
                None => "› Diagnose erstellt".into(),
            }),
            3 if code => lines.push("› Wende den Fix an, starte Lint und Tests … bestanden".into()),
            3 => lines.push("› Kein Code-Fix nötig, der Branch bleibt unverändert".into()),
            _ if code => lines.push(format!(
                "› git push origin {}, starte „{check}“ neu",
                self.branch()
            )),
            _ => lines.push(format!("› Starte „{check}“ neu")),
        }
    }

    /// Log up to and including the step the agent is currently working on.
    pub fn log(&self) -> String {
        if self.real || self.ended.is_some() {
            return self.real_log();
        }
        let shown = if self.is_awaiting() {
            1
        } else if self.is_diagnosed() {
            self.completed
        } else if self.is_working() {
            self.completed + 1
        } else {
            self.completed
        }
        .min(STEPS);
        if shown == 0 {
            return "Noch keine Aktivität. Starte den Agenten, um das Ticket umzusetzen.".into();
        }
        let o = &self.options;
        let hint = o.hint.trim();
        let mut lines = Vec::new();
        for step in 0..shown {
            match (&self.kind, step) {
                (RunKind::AddressReview { review, comments }, _) => {
                    self.address_log(&mut lines, step, review, comments.len());
                }
                (RunKind::Diagnose { review, check }, _) => {
                    self.diagnose_log(&mut lines, step, review, check);
                }
                (RunKind::Followup { review, .. }, _) => {
                    self.address_log(&mut lines, step, review, 0);
                }
                (RunKind::Implement, 0) => {
                    let comments = match self.comments {
                        1 => "1 Kommentar".to_string(),
                        n => format!("{n} Kommentare"),
                    };
                    lines.push(format!("› Lese {} und {comments}", self.ticket_key));
                    if !hint.is_empty() {
                        lines.push(format!("› Hinweis: {hint}"));
                    }
                    for rule in self.rules.lines() {
                        lines.push(format!("› Regel: {rule}"));
                    }
                    if let Some(plan) = self.plan() {
                        lines.push(format!(
                            "› Plan erstellt: {} Schritte, {} Dateien",
                            plan.steps.len(),
                            plan.files.len()
                        ));
                        for v in &plan.violations {
                            lines.push(format!("› Regelverstoß: {}", v.reason()));
                        }
                        for note in &self.plan_notes {
                            lines.push(format!("› Neuer Plan angefordert: {note}"));
                        }
                        lines.push(if self.is_awaiting() {
                            "› Warte auf Freigabe".into()
                        } else {
                            "› Plan freigegeben".into()
                        });
                    }
                }
                (RunKind::Implement, 1) => lines.push(format!("› git switch -c {}", self.branch())),
                (RunKind::Implement, 2) => {
                    lines.push(format!("› Bearbeite {}", self.files.join(", ")))
                }
                (RunKind::Implement, 3) if !self.rules.required_tests.is_empty() => {
                    for cmd in &self.rules.required_tests {
                        lines.push(format!("› $ {cmd} … bestanden"));
                    }
                }
                (RunKind::Implement, 3) if o.run_tests => {
                    lines.push("› Starte Lint und Tests … bestanden".into())
                }
                (RunKind::Implement, 3) => lines.push("› Tests übersprungen".into()),
                (RunKind::Implement, _) => lines.push(format!(
                    "› Pull Request {} erstellt{}",
                    self.pr_label(),
                    if o.draft_pr { " (Entwurf)" } else { "" }
                )),
            }
            // Questions, answers and acknowledged hints of this step.
            for m in self.messages.iter().filter(|m| m.step == step) {
                match m.kind {
                    MessageKind::Question => lines.push(format!("› Frage: {}", m.text)),
                    MessageKind::Answer => lines.push(format!("› Antwort: {}", m.text)),
                    MessageKind::Ack => lines.push(format!("› {}", m.text)),
                    _ => {}
                }
            }
        }
        if self.is_diagnosed() {
            lines.push("› Warte auf deine Entscheidung über den Fix".into());
        } else if self.is_asking() {
            lines.push("› Warte auf deine Antwort".into());
        } else if self.is_paused() {
            lines.push("› Pausiert".into());
        } else if let Some(reason) = self.stop_reason() {
            lines.push(format!("› Angehalten: {}", reason.label()));
        }
        lines.join("\n")
    }
}

pub(super) fn plural_comments(n: usize) -> String {
    match n {
        1 => "1 Kommentar".to_string(),
        n => format!("{n} Kommentare"),
    }
}

pub(super) fn files_label(n: usize) -> String {
    match n {
        1 => "1 Datei".to_string(),
        n => format!("{n} Dateien"),
    }
}
