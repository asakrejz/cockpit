use super::log::{files_label, plural_comments};
use super::*;

impl AgentRun {
    pub(super) fn pr_label(&self) -> String {
        self.pr_number
            .map_or("Pull Request".to_string(), |n| format!("#{n}"))
    }

    /// Title and text of each step, matching kind and options of the run.
    pub fn steps(&self) -> Vec<Step> {
        let texts = match &self.kind {
            RunKind::Implement => self.implement_texts(),
            RunKind::AddressReview { review, comments } => {
                self.address_texts(review, comments.len())
            }
            RunKind::Diagnose { review, check } => self.diagnose_texts(review, check),
        };
        self.build_steps(texts)
    }

    pub(super) fn address_texts(
        &self,
        review: &PrKey,
        comments: usize,
    ) -> [(&'static str, String); STEPS] {
        let o = &self.options;
        [
            (
                "Kommentare gelesen",
                format!(
                    "{} aus Review {review} ausgewertet",
                    plural_comments(comments)
                ),
            ),
            ("Branch ausgecheckt", self.branch()),
            (
                "Änderungen umgesetzt",
                files_label(self.files.len()) + " angepasst",
            ),
            if o.run_tests {
                (
                    "Tests ausgeführt",
                    "Lint und Tests laufen im Branch".to_string(),
                )
            } else {
                (
                    "Tests übersprungen",
                    "In den Optionen abgeschaltet".to_string(),
                )
            },
            (
                "Auf Branch gepusht",
                format!(
                    "Neuer Commit auf {} – kein neuer Pull Request, das Review wartet wieder auf dich",
                    self.branch()
                ),
            ),
        ]
    }

    pub(super) fn diagnose_texts(
        &self,
        review: &PrKey,
        check: &str,
    ) -> [(&'static str, String); STEPS] {
        let code = self.diagnosis.as_ref().is_none_or(Diagnosis::changes_code);
        [
            (
                "Log gelesen",
                format!("Check „{check}“ von {review}: Ausgabe ausgewertet"),
            ),
            (
                "Ursache eingegrenzt",
                "Fehlermeldung, Datei und Zeile bestimmt".to_string(),
            ),
            (
                "Diagnose erstellt",
                "Ursache und Vorschlag liegen zur Entscheidung vor".to_string(),
            ),
            if code {
                (
                    "Fix angewendet",
                    "Änderung im Branch, Lint und Tests laufen".to_string(),
                )
            } else {
                (
                    "Kein Code-Fix nötig",
                    "Der Branch bleibt unverändert".to_string(),
                )
            },
            if code {
                (
                    "Gepusht, Check neu gestartet",
                    format!("{} aktualisiert, „{check}“ läuft erneut", self.branch()),
                )
            } else {
                ("Check neu gestartet", format!("„{check}“ läuft erneut"))
            },
        ]
    }

    pub(super) fn implement_texts(&self) -> [(&'static str, String); STEPS] {
        let o = &self.options;
        let mut files = match self.files.len() {
            1 => "1 Datei angepasst".to_string(),
            n => format!("{n} Dateien angepasst"),
        };
        if self.rules.has_path_rules() && self.violations().is_empty() {
            files.push_str(" · Pfad-Regeln eingehalten");
        }
        [
            (
                "Ticket gelesen",
                format!(
                    "Beschreibung und Kommentare aus {} ausgewertet",
                    self.ticket_key
                ),
            ),
            (
                "Branch erstellt",
                format!("{} von {}", self.branch(), o.base_branch),
            ),
            ("Änderungen umgesetzt", files),
            if !self.rules.required_tests.is_empty() {
                (
                    "Pflicht-Tests ausgeführt",
                    format!(
                        "{} im Branch bestanden",
                        self.rules.required_tests.join(", ")
                    ),
                )
            } else if o.run_tests {
                (
                    "Tests ausgeführt",
                    "Lint und Tests laufen im Branch".to_string(),
                )
            } else {
                (
                    "Tests übersprungen",
                    "In den Optionen abgeschaltet".to_string(),
                )
            },
            (
                "Pull Request erstellt",
                format!(
                    "{}{} – wartet auf dein Review",
                    self.pr_label(),
                    if o.draft_pr { " als Entwurf" } else { "" }
                ),
            ),
        ]
    }

    pub(super) fn build_steps(&self, texts: [(&'static str, String); STEPS]) -> Vec<Step> {
        texts
            .into_iter()
            .enumerate()
            .map(|(i, (title, text))| {
                if i == 0 && self.is_awaiting() {
                    return Step {
                        title: "Plan liegt vor".to_string(),
                        text: if self.violations().is_empty() {
                            "Wartet auf deine Freigabe – bis dahin ändert der Agent nichts"
                        } else {
                            "Der Plan verstößt gegen die Regeln des Repos – er lässt sich nicht freigeben"
                        }
                        .to_string(),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                if i == self.completed && self.is_diagnosed() {
                    return Step {
                        title: "Diagnose liegt vor".to_string(),
                        text: "Wartet auf deine Entscheidung – bis dahin bleibt der Branch unverändert"
                            .to_string(),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                if i == self.completed && self.is_asking() {
                    return Step {
                        title: "Frage an dich".to_string(),
                        text: "Wartet auf deine Antwort".to_string(),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                if i == self.completed && self.is_paused() {
                    return Step {
                        title: title.to_string(),
                        text: "Pausiert – setze den Lauf fort, damit es weitergeht".to_string(),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                if i == self.completed
                    && let Some(reason) = self.stop_reason()
                {
                    return Step {
                        title: title.to_string(),
                        text: format!("Angehalten: {} – erhöhe das Limit oder brich ab", reason.label()),
                        state: StepState::Active,
                        usage: None,
                    };
                }
                // If the user answered a question at this step, it is noted there.
                let text = match self
                    .messages
                    .iter()
                    .find(|m| m.kind == MessageKind::Answer && m.step == i)
                {
                    Some(answer) => format!("{text} · Deine Antwort: {}", answer.text),
                    None => text,
                };
                Step {
                    title: title.to_string(),
                    text,
                    usage: if i < self.completed {
                        self.step_usage.get(i).copied()
                    } else {
                        None
                    },
                    state: if i < self.completed {
                        StepState::Done
                    } else if i == self.completed && self.is_working() {
                        StepState::Active
                    } else {
                        StepState::Pending
                    },
                }
            })
            .collect()
    }
}
