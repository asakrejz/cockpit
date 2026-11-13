use super::log::{files_label, plural_comments};
use super::*;

impl AgentRun {
    /// The current plan. Before the start there is none.
    pub fn plan(&self) -> Option<Plan> {
        if !self.planned {
            return None;
        }
        if let Some(p) = &self.agent_plan {
            return Some(Plan {
                summary: p.summary.clone(),
                files: p.files.clone(),
                steps: p.steps.clone(),
                rules: self.rules.lines(),
                violations: self.violations(),
            });
        }
        if let RunKind::AddressReview { review, comments } = &self.kind {
            return Some(self.address_plan(review, comments.len()));
        }
        let o = &self.options;
        let hint = o.hint.trim();
        let files = match self.files.len() {
            1 => "1 Datei".to_string(),
            n => format!("{n} Dateien"),
        };
        let mut summary = format!(
            "{} umsetzen: {files} ändern{}.",
            self.ticket_key,
            if o.run_tests {
                ", Tests ausführen"
            } else {
                " (ohne Tests)"
            }
        );
        if !hint.is_empty() {
            summary.push_str(&format!(" Dein Hinweis: „{hint}“."));
        }
        let mut steps = vec![
            format!(
                "Ticket {} und {} Kommentar(e) auswerten",
                self.ticket_key, self.comments
            ),
            format!("Branch {} von {} erstellen", self.branch(), o.base_branch),
            format!("{} bearbeiten", self.files.join(", ")),
        ];
        for note in &self.plan_notes {
            steps.push(format!("Berücksichtigen: {note}"));
        }
        steps.push(if !self.rules.required_tests.is_empty() {
            format!(
                "Pflicht-Tests ausführen: {}",
                self.rules.required_tests.join(", ")
            )
        } else if o.run_tests {
            "Lint und Tests ausführen".into()
        } else {
            "Tests überspringen".into()
        });
        steps.push(format!(
            "Pull Request {}anlegen – ohne Merge",
            if o.draft_pr { "als Entwurf " } else { "" }
        ));
        if !self.plan_notes.is_empty() {
            summary.push_str(&format!(" Überarbeitet ({}×).", self.plan_notes.len()));
        }
        Some(Plan {
            summary,
            files: self.files.clone(),
            steps,
            rules: self.rules.lines(),
            violations: self.violations(),
        })
    }

    /// Plan for „implement review comments“: no new branch, no new pull request.
    pub(super) fn address_plan(&self, review: &PrKey, comments: usize) -> Plan {
        let o = &self.options;
        let hint = o.hint.trim();
        let branch = self.branch();
        let mut summary = format!(
            "{} aus Review {review} umsetzen: {} ändern{}, auf {branch} pushen.",
            plural_comments(comments),
            files_label(self.files.len()),
            if o.run_tests {
                ", Tests ausführen"
            } else {
                " (ohne Tests)"
            }
        );
        if !hint.is_empty() {
            summary.push_str(&format!(" Dein Hinweis: „{hint}“."));
        }
        let mut steps = vec![
            format!("{} aus Review {review} lesen", plural_comments(comments)),
            format!("Branch {branch} auschecken"),
            format!("{} bearbeiten", self.files.join(", ")),
        ];
        for note in &self.plan_notes {
            steps.push(format!("Berücksichtigen: {note}"));
        }
        steps.push(if !self.rules.required_tests.is_empty() {
            format!(
                "Pflicht-Tests ausführen: {}",
                self.rules.required_tests.join(", ")
            )
        } else if o.run_tests {
            "Lint und Tests ausführen".into()
        } else {
            "Tests überspringen".into()
        });
        steps.push(format!(
            "Auf {branch} pushen – kein neuer Pull Request, kein Merge"
        ));
        if !self.plan_notes.is_empty() {
            summary.push_str(&format!(" Überarbeitet ({}×).", self.plan_notes.len()));
        }
        Plan {
            summary,
            files: self.files.clone(),
            steps,
            rules: self.rules.lines(),
            violations: self.violations(),
        }
    }
}
