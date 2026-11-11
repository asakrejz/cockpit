//! Ein Agent setzt ein Ticket um: lesen, Branch, Änderungen, Tests, Pull Request.
//!
//! Der Lauf ist eine kleine Zustandsmaschine. Wer sie antreibt – in der Demo ein
//! Timer, später ein Backend –, ruft nur [`AgentRun::advance`] auf.

use crate::text::slug;

/// Anzahl der Schritte eines Laufs.
pub const STEPS: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentOptions {
    pub run_tests: bool,
    /// Pull Request als Entwurf anlegen.
    pub draft_pr: bool,
    /// Zusätzliche Hinweise an den Agenten.
    pub hint: String,
    pub base_branch: String,
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            run_tests: true,
            draft_pr: true,
            hint: String::new(),
            base_branch: "main".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunState {
    #[default]
    Idle,
    Running,
    Done,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRun {
    pub ticket_key: String,
    pub ticket_title: String,
    /// Kommentare am Ticket, die der Agent mitliest.
    pub comments: usize,
    /// Namen der Dateien, die der Agent ändert.
    pub files: Vec<String>,
    pub options: AgentOptions,
    pub state: RunState,
    /// Zahl der abgeschlossenen Schritte (0 bis [`STEPS`]).
    pub completed: usize,
    /// Nummer des Pull Requests, den der Lauf anlegt.
    pub pr_number: Option<u32>,
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
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state == RunState::Idle
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
        self.options = options;
        self.state = RunState::Running;
        self.completed = 0;
        self.pr_number = Some(pr_number);
    }

    /// Bricht ab oder setzt zurück: Der Lauf ist danach wieder startbereit.
    pub fn reset(&mut self) {
        self.state = RunState::Idle;
        self.completed = 0;
        self.pr_number = None;
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
            .map(|(i, (title, text))| Step {
                title: title.to_string(),
                text,
                state: if i < self.completed {
                    StepState::Done
                } else if i == self.completed && self.is_running() {
                    StepState::Active
                } else {
                    StepState::Pending
                },
            })
            .collect()
    }

    /// Protokoll bis einschließlich des Schritts, an dem der Agent gerade arbeitet.
    pub fn log(&self) -> String {
        let shown = if self.is_running() {
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
        r.start(AgentOptions::default(), 92);
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
            ..Default::default()
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
        r.start(AgentOptions::default(), 92);
        r.advance();
        r.reset();
        assert!(r.is_idle());
        assert_eq!((r.completed, r.pr_number), (0, None));
    }
}
