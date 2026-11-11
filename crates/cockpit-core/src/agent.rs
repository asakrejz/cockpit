//! Ein Agent setzt ein Ticket um: lesen, Branch, Änderungen, Tests, Pull Request.
//!
//! Der Lauf ist eine kleine Zustandsmaschine. Wer sie antreibt – in der Demo ein
//! Timer, später ein Backend –, ruft nur [`AgentRun::advance`] auf.
//!
//! Auf Wunsch legt der Agent vorher einen [`Plan`] vor und wartet auf die Freigabe
//! ([`RunState::AwaitingApproval`]). Bis dahin entsteht nichts: kein Branch, keine Änderung.

use serde::{Deserialize, Serialize};

use crate::text::slug;

/// Anzahl der Schritte eines Laufs.
pub const STEPS: usize = 5;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct AgentOptions {
    pub run_tests: bool,
    /// Pull Request als Entwurf anlegen.
    pub draft_pr: bool,
    /// Zusätzliche Hinweise an den Agenten.
    pub hint: String,
    pub base_branch: String,
    /// Erst einen Plan vorlegen und auf die Freigabe warten.
    pub plan_first: bool,
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            run_tests: true,
            draft_pr: true,
            hint: String::new(),
            base_branch: "main".into(),
            plan_first: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunState {
    #[default]
    Idle,
    /// Der Plan liegt vor, der Agent wartet auf den Nutzer.
    AwaitingApproval,
    Running,
    Done,
}

/// Was der Agent vorhat. Wird aus Ticket, Hinweisen und Dateien abgeleitet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub summary: String,
    pub files: Vec<String>,
    pub steps: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Pending,
    Active,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub title: String,
    pub text: String,
    pub state: StepState,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentRun {
    #[serde(default)]
    pub ticket_key: String,
    #[serde(default)]
    pub ticket_title: String,
    /// Kommentare am Ticket, die der Agent mitliest.
    #[serde(default)]
    pub comments: usize,
    /// Namen der Dateien, die der Agent ändert.
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub options: AgentOptions,
    #[serde(default)]
    pub state: RunState,
    /// Zahl der abgeschlossenen Schritte (0 bis [`STEPS`]).
    #[serde(default)]
    pub completed: usize,
    /// Nummer des Pull Requests, den der Lauf anlegt.
    #[serde(default)]
    pub pr_number: Option<u32>,
    /// Anmerkungen des Nutzers, die zu einem neuen Plan geführt haben.
    pub plan_notes: Vec<String>,
    /// Der Lauf hat mit einem Plan begonnen (auch nach der Freigabe).
    pub planned: bool,
}

impl AgentRun {
    pub fn new(ticket_key: &str, ticket_title: &str) -> Self {
        Self {
            ticket_key: ticket_key.into(),
            ticket_title: ticket_title.into(),
            comments: 0,
            files: Vec::new(),
            options: AgentOptions::default(),
            state: RunState::Idle,
            completed: 0,
            pr_number: None,
            plan_notes: Vec::new(),
            planned: false,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state == RunState::Idle
    }

    pub fn is_awaiting(&self) -> bool {
        self.state == RunState::AwaitingApproval
    }

    /// Wartet auf Freigabe oder läuft – der Lauf belegt das Ticket.
    pub fn is_active(&self) -> bool {
        self.is_awaiting() || self.is_running()
    }

    pub fn is_running(&self) -> bool {
        self.state == RunState::Running
    }

    pub fn is_done(&self) -> bool {
        self.state == RunState::Done
    }

    /// `agent/BAU-142-toggle-fokus-ring-fehlt`
    pub fn branch(&self) -> String {
        format!("agent/{}-{}", self.ticket_key, slug(&self.ticket_title, 24))
    }

    pub fn start(&mut self, options: AgentOptions, pr_number: u32) {
        self.planned = options.plan_first;
        self.state = if self.planned {
            RunState::AwaitingApproval
        } else {
            RunState::Running
        };
        self.options = options;
        self.completed = 0;
        self.pr_number = Some(pr_number);
        self.plan_notes.clear();
    }

    /// Bricht ab oder setzt zurück: Der Lauf ist danach wieder startbereit.
    pub fn reset(&mut self) {
        self.state = RunState::Idle;
        self.completed = 0;
        self.pr_number = None;
        self.plan_notes.clear();
        self.planned = false;
    }

    /// Gibt den Plan frei: Ab jetzt arbeitet der Agent. `false`, wenn es nichts freizugeben gibt.
    pub fn approve(&mut self) -> bool {
        if !self.is_awaiting() {
            return false;
        }
        self.state = RunState::Running;
        true
    }

    /// Lehnt den Plan ab. Der Lauf endet ohne Spuren, wie beim Abbrechen.
    pub fn reject(&mut self) -> bool {
        if !self.is_awaiting() {
            return false;
        }
        self.reset();
        true
    }

    /// Fordert mit einer Anmerkung einen neuen Plan an. Leere Anmerkungen zählen nicht.
    pub fn revise(&mut self, note: &str) -> bool {
        let note = note.trim();
        if !self.is_awaiting() || note.is_empty() {
            return false;
        }
        self.plan_notes.push(note.to_string());
        true
    }

    /// Der aktuelle Plan. Vor dem Start gibt es keinen.
    pub fn plan(&self) -> Option<Plan> {
        if !self.planned {
            return None;
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
        steps.push(if o.run_tests {
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
        })
    }

    /// Schließt den aktuellen Schritt ab. Gibt `true` zurück, wenn der Lauf damit
    /// fertig geworden ist.
    pub fn advance(&mut self) -> bool {
        if !self.is_running() {
            return false;
        }
        self.completed += 1;
        if self.completed == STEPS {
            self.state = RunState::Done;
        }
        self.is_done()
    }

    fn pr_label(&self) -> String {
        self.pr_number
            .map_or("Pull Request".to_string(), |n| format!("#{n}"))
    }

    /// Titel und Text jedes Schritts, passend zu den Optionen.
    pub fn steps(&self) -> Vec<Step> {
        let o = &self.options;
        let files = match self.files.len() {
            1 => "1 Datei angepasst".to_string(),
            n => format!("{n} Dateien angepasst"),
        };
        let texts: [(&str, String); STEPS] = [
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
                "Pull Request erstellt",
                format!(
                    "{}{} – wartet auf dein Review",
                    self.pr_label(),
                    if o.draft_pr { " als Entwurf" } else { "" }
                ),
            ),
        ];
        texts
            .into_iter()
            .enumerate()
            .map(|(i, (title, text))| {
                if i == 0 && self.is_awaiting() {
                    return Step {
                        title: "Plan liegt vor".to_string(),
                        text: "Wartet auf deine Freigabe – bis dahin ändert der Agent nichts"
                            .to_string(),
                        state: StepState::Active,
                    };
                }
                Step {
                    title: title.to_string(),
                    text,
                    state: if i < self.completed {
                        StepState::Done
                    } else if i == self.completed && self.is_running() {
                        StepState::Active
                    } else {
                        StepState::Pending
                    },
                }
            })
            .collect()
    }

    /// Protokoll bis einschließlich des Schritts, an dem der Agent gerade arbeitet.
    pub fn log(&self) -> String {
        let shown = if self.is_awaiting() {
            1
        } else if self.is_running() {
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
            match step {
                0 => {
                    let comments = match self.comments {
                        1 => "1 Kommentar".to_string(),
                        n => format!("{n} Kommentare"),
                    };
                    lines.push(format!("› Lese {} und {comments}", self.ticket_key));
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
                1 => lines.push(format!("› git switch -c {}", self.branch())),
                2 => lines.push(format!("› Bearbeite {}", self.files.join(", "))),
                3 if o.run_tests => lines.push("› Starte Lint und Tests … bestanden".into()),
                3 => lines.push("› Tests übersprungen".into()),
                _ => lines.push(format!(
                    "› Pull Request {} erstellt{}",
                    self.pr_label(),
                    if o.draft_pr { " (Entwurf)" } else { "" }
                )),
            }
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ohne Plan-Freigabe: der Lauf startet sofort.
    fn direct() -> AgentOptions {
        AgentOptions {
            plan_first: false,
            ..Default::default()
        }
    }

    fn run() -> AgentRun {
        let mut r = AgentRun::new("BAU-142", "Toggle: Fokus-Ring fehlt im Dark Mode");
        r.comments = 1;
        r.files = vec!["components.css".into(), "tokens.css".into()];
        r
    }

    #[test]
    fn a_new_run_is_idle() {
        let r = run();
        assert!(r.is_idle());
        assert!(r.log().starts_with("Noch keine Aktivität"));
        assert!(r.steps().iter().all(|s| s.state == StepState::Pending));
        assert_eq!(r.branch(), "agent/BAU-142-toggle-fokus-ring-fehlt");
    }

    #[test]
    fn advance_only_works_while_running() {
        let mut r = run();
        assert!(!r.advance());
        assert_eq!(r.completed, 0);
    }

    #[test]
    fn exactly_one_step_is_active_while_running() {
        let mut r = run();
        r.start(direct(), 92);
        for done in 0..STEPS {
            let states: Vec<_> = r.steps().into_iter().map(|s| s.state).collect();
            assert_eq!(
                states.iter().filter(|s| **s == StepState::Done).count(),
                done
            );
            assert_eq!(
                states.iter().filter(|s| **s == StepState::Active).count(),
                1
            );
            assert_eq!(states[done], StepState::Active);
            assert_eq!(r.log().lines().count(), done + 1);
            let finished = r.advance();
            assert_eq!(finished, done + 1 == STEPS);
        }
        assert!(r.is_done());
        assert!(r.steps().iter().all(|s| s.state == StepState::Done));
        assert!(r.log().ends_with("› Pull Request #92 erstellt (Entwurf)"));
        assert!(!r.advance(), "ein fertiger Lauf bleibt fertig");
        assert_eq!(r.completed, STEPS);
    }

    #[test]
    fn options_show_up_in_steps_and_log() {
        let mut r = run();
        let options = AgentOptions {
            run_tests: false,
            draft_pr: false,
            hint: " Auch den Slider prüfen ".into(),
            ..direct()
        };
        r.start(options, 7);
        while !r.advance() {}
        let steps = r.steps();
        assert_eq!(steps[3].title, "Tests übersprungen");
        assert_eq!(steps[4].text, "#7 – wartet auf dein Review");
        let log = r.log();
        assert!(log.contains("› Hinweis: Auch den Slider prüfen\n"), "{log}");
        assert!(log.contains("› Tests übersprungen"));
        assert!(log.contains("› Bearbeite components.css, tokens.css"));
        assert_eq!(
            log.lines().count(),
            STEPS + 1,
            "der Hinweis ist eine Zusatzzeile"
        );
    }

    #[test]
    fn reset_makes_the_run_startable_again() {
        let mut r = run();
        r.start(direct(), 92);
        r.advance();
        r.reset();
        assert!(r.is_idle());
        assert_eq!((r.completed, r.pr_number), (0, None));
    }

    #[test]
    fn a_plan_waits_for_approval() {
        let mut r = run();
        r.options.hint = "Auch den Slider prüfen".into();
        let options = r.options.clone();
        r.start(options, 92);
        assert!(r.is_awaiting() && r.is_active() && !r.is_running());
        assert!(!r.advance(), "ohne Freigabe kein Fortschritt");
        assert_eq!(r.completed, 0);
        let plan = r.plan().unwrap();
        assert_eq!(plan.files, ["components.css", "tokens.css"]);
        assert!(plan.summary.contains("Auch den Slider prüfen"));
        assert!(plan.steps.iter().any(|s| s.contains("agent/BAU-142")));
        let steps = r.steps();
        assert_eq!(steps[0].state, StepState::Active);
        assert_eq!(steps[0].title, "Plan liegt vor");
        assert!(steps[1..].iter().all(|s| s.state == StepState::Pending));
        assert!(r.log().ends_with("› Warte auf Freigabe"), "{}", r.log());
        // Es steht noch kein Branch im Protokoll.
        assert!(!r.log().contains("git switch"));

        assert!(r.approve());
        assert!(!r.approve());
        assert!(r.log().ends_with("› Plan freigegeben"), "{}", r.log());
        assert!(r.advance() || r.completed == 1);
    }

    #[test]
    fn rejecting_resets_the_run() {
        let mut r = run();
        r.start(AgentOptions::default(), 92);
        assert!(r.revise("noch eine Anmerkung"));
        assert!(r.reject());
        assert!(r.is_idle() && r.plan().is_none());
        assert_eq!(
            (r.pr_number, r.plan_notes.len(), r.planned),
            (None, 0, false)
        );
        assert!(!r.reject());
        assert!(!r.approve());
    }

    #[test]
    fn a_note_creates_a_new_plan() {
        let mut r = run();
        r.start(AgentOptions::default(), 92);
        let before = r.plan().unwrap();
        assert!(!r.revise("   "));
        assert!(r.revise("  Bitte auch Tests für den Slider  "));
        let after = r.plan().unwrap();
        assert_ne!(before, after);
        assert_eq!(after.steps.len(), before.steps.len() + 1);
        assert!(
            after
                .steps
                .iter()
                .any(|s| s == "Berücksichtigen: Bitte auch Tests für den Slider")
        );
        assert!(r.is_awaiting(), "der neue Plan braucht wieder Freigabe");
        assert!(
            r.log()
                .contains("› Neuer Plan angefordert: Bitte auch Tests für den Slider")
        );
        // Nach der Freigabe lässt sich nicht mehr überarbeiten.
        r.approve();
        assert!(!r.revise("zu spät"));
    }

    #[test]
    fn without_plan_first_the_agent_runs_right_away() {
        let mut r = run();
        r.start(direct(), 92);
        assert!(r.is_running() && r.plan().is_none());
        assert!(!r.approve());
        assert!(!r.log().contains("Plan"));
    }
}
