//! Ein Agent setzt ein Ticket um: lesen, Branch, Änderungen, Tests, Pull Request.
//!
//! Der Lauf ist eine kleine Zustandsmaschine. Wer sie antreibt – in der Demo ein
//! Timer, später ein Backend –, ruft nur [`AgentRun::advance`] auf.
//!
//! Auf Wunsch legt der Agent vorher einen [`Plan`] vor und wartet auf die Freigabe
//! ([`RunState::AwaitingApproval`]). Bis dahin entsteht nichts: kein Branch, keine Änderung.
//!
//! Während des Laufs kann der Agent eine [`Question`] stellen ([`RunState::AwaitingAnswer`]),
//! der Nutzer kann ihm Hinweise schicken ([`AgentRun::steer`]) und ihn pausieren
//! ([`RunState::Paused`]). Alles Gesagte steht im Gesprächsverlauf ([`AgentRun::messages`]).
//!
//! Jeder Schritt hat einen Verbrauch ([`AgentRun::step_usage`]). Erreicht ein Lauf eine Grenze
//! ([`AgentLimits`]), hält er an ([`RunState::Stopped`]) und wartet darauf, dass du das Limit
//! erhöhst und ihn fortsetzt – oder abbrichst. Die Regeln des Repos ([`RepoRules`]) gelten ab dem
//! Start: Sie stehen im Plan, und ein Plan, der verbotene Pfade berührt, lässt sich nicht freigeben.

use serde::{Deserialize, Serialize};

use crate::rules::{PathViolation, RepoRules};
use crate::text::slug;
use crate::usage::{AgentLimits, StopReason, Usage};

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
    /// Der Agent hat eine Frage gestellt und wartet auf die Antwort.
    AwaitingAnswer,
    /// Der Nutzer hat den Lauf angehalten.
    Paused,
    /// Eine Grenze (Budget, Zeitlimit) ist erreicht: Der Lauf wartet auf dich.
    Stopped(StopReason),
    Done,
}

/// Eine Rückfrage des Agenten, optional mit vorgeschlagenen Antworten.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Question {
    pub text: String,
    #[serde(default)]
    pub suggestions: Vec<String>,
}

/// Wer im Verlauf spricht.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    Agent,
    Du,
}

/// Was eine Meldung im Verlauf ist.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    /// Normale Meldung (Auftrag, Schritt abgeschlossen).
    Info,
    Question,
    Answer,
    /// Hinweis des Nutzers, auch eine Anmerkung zum Plan.
    Hint,
    /// Der Agent bestätigt, dass er einen Hinweis berücksichtigt.
    Ack,
}

/// Eine Zeile im Gesprächsverlauf eines Laufs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RunMessage {
    pub from: Speaker,
    pub kind: MessageKind,
    /// Zahl der abgeschlossenen Schritte, als die Meldung entstand.
    pub step: usize,
    pub text: String,
}

/// Was ein Takt bewirkt hat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Der Lauf läuft nicht, nichts geschah.
    Idle,
    /// Der Agent hat angehalten und eine Frage gestellt.
    Asked,
    /// Ein Schritt wurde abgeschlossen.
    Stepped,
    /// Der letzte Schritt wurde abgeschlossen.
    Finished,
    /// Eine Grenze ist erreicht: Der Lauf hat angehalten, ohne einen Schritt zu machen.
    Stopped,
}

/// Was der Agent vorhat. Wird aus Ticket, Hinweisen und Dateien abgeleitet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub summary: String,
    pub files: Vec<String>,
    pub steps: Vec<String>,
    /// Die Regeln des Repos, die für diesen Lauf gelten (eine Zeile je Regel).
    pub rules: Vec<String>,
    /// Dateien des Plans, die gegen die Pfad-Regeln verstoßen. Solange es welche gibt, ist der
    /// Plan nicht freigebbar.
    pub violations: Vec<PathViolation>,
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
    /// Verbrauch des Schritts, sobald er abgeschlossen ist.
    pub usage: Option<Usage>,
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
    #[serde(default)]
    pub plan_notes: Vec<String>,
    /// Der Lauf hat mit einem Plan begonnen (auch nach der Freigabe).
    #[serde(default)]
    pub planned: bool,
    /// Gesprächsverlauf: Meldungen des Agenten, Fragen, Antworten und Hinweise.
    #[serde(default)]
    pub messages: Vec<RunMessage>,
    /// Die offene Frage, solange der Lauf auf eine Antwort wartet.
    #[serde(default)]
    pub question: Option<Question>,
    /// Die Frage wurde in diesem Lauf schon gestellt (sie kommt nur einmal).
    #[serde(default)]
    pub asked: bool,
    /// Hinweise, die der Agent beim nächsten Schritt berücksichtigt.
    #[serde(default)]
    pub pending_hints: Vec<String>,
    /// Das Repo, in dem der Lauf arbeitet (Schlüssel der Regeln).
    #[serde(default)]
    pub repo: String,
    /// Die Regeln des Repos, wie sie beim Start (bzw. beim Planen) galten.
    #[serde(default)]
    pub rules: RepoRules,
    /// Verbrauch je abgeschlossenem Schritt (Index = Schritt).
    #[serde(default)]
    pub step_usage: Vec<Usage>,
    /// Beim Fortsetzen zusätzlich erlaubtes Budget dieses Laufs, in Cent.
    #[serde(default)]
    pub extra_budget_cents: u64,
    /// Beim Fortsetzen zusätzlich erlaubte Minuten dieses Laufs.
    #[serde(default)]
    pub extra_minutes: u64,
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
            messages: Vec::new(),
            question: None,
            asked: false,
            pending_hints: Vec::new(),
            repo: String::new(),
            rules: RepoRules::default(),
            step_usage: Vec::new(),
            extra_budget_cents: 0,
            extra_minutes: 0,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state == RunState::Idle
    }

    pub fn is_awaiting(&self) -> bool {
        self.state == RunState::AwaitingApproval
    }

    /// Wartet auf die Antwort auf eine Rückfrage.
    pub fn is_asking(&self) -> bool {
        self.state == RunState::AwaitingAnswer
    }

    pub fn is_paused(&self) -> bool {
        self.state == RunState::Paused
    }

    /// Eine Grenze hat den Lauf angehalten.
    pub fn is_stopped(&self) -> bool {
        matches!(self.state, RunState::Stopped(_))
    }

    /// Welche Grenze den Lauf angehalten hat.
    pub fn stop_reason(&self) -> Option<StopReason> {
        match self.state {
            RunState::Stopped(reason) => Some(reason),
            _ => None,
        }
    }

    /// Wartet, fragt, pausiert, ist angehalten oder läuft – der Lauf belegt das Ticket.
    pub fn is_active(&self) -> bool {
        self.is_awaiting()
            || self.is_running()
            || self.is_asking()
            || self.is_paused()
            || self.is_stopped()
    }

    /// Der Agent arbeitet an den Schritten (läuft, fragt, ist pausiert oder angehalten).
    fn is_working(&self) -> bool {
        self.is_running() || self.is_asking() || self.is_paused() || self.is_stopped()
    }

    /// Der Lauf verbraucht gerade Ressourcen: Er arbeitet oder wartet mitten im Schritt auf eine
    /// Antwort. Nur diese Läufe zählen für die Grenze paralleler Läufe.
    pub fn is_consuming(&self) -> bool {
        self.is_running() || self.is_asking()
    }

    /// Verbrauch des ganzen Laufs (Summe der Schritte).
    pub fn usage(&self) -> Usage {
        let mut sum = Usage::default();
        for u in &self.step_usage {
            sum.add(u);
        }
        sum
    }

    /// Trägt den Verbrauch des Schritts ein, an dem der Agent gerade arbeitet. In der Demo macht
    /// das der Takt; der echte Agent meldet hier die Werte aus dem Lauf. Mehrere Meldungen für
    /// denselben Schritt addieren sich.
    pub fn record_usage(&mut self, usage: Usage) {
        while self.step_usage.len() <= self.completed {
            self.step_usage.push(Usage::default());
        }
        self.step_usage[self.completed].add(&usage);
    }

    /// Dateien des Laufs, die gegen die Pfad-Regeln verstoßen.
    pub fn violations(&self) -> Vec<PathViolation> {
        self.rules.violations(&self.files)
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

    pub fn start(&mut self, mut options: AgentOptions, pr_number: u32) {
        // Die Regeln des Repos gehen vor: Pflicht-Tests lassen sich nicht abschalten, und wer
        // gegen die Pfad-Regeln verstößt oder dessen Repo es verlangt, legt immer einen Plan vor.
        if !self.rules.required_tests.is_empty() {
            options.run_tests = true;
        }
        if self.rules.plan_required || !self.violations().is_empty() {
            options.plan_first = true;
        }
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
        self.clear_usage();
        self.clear_conversation();
        self.say(
            Speaker::Agent,
            MessageKind::Info,
            format!("Auftrag für {} angenommen.", self.ticket_key),
        );
        let hint = self.options.hint.trim().to_string();
        if !hint.is_empty() {
            self.say(Speaker::Du, MessageKind::Hint, hint);
        }
        if self.planned {
            self.say(
                Speaker::Agent,
                MessageKind::Info,
                "Ich habe einen Plan vorgelegt und warte auf deine Freigabe.".into(),
            );
        }
    }

    fn clear_usage(&mut self) {
        self.step_usage.clear();
        self.extra_budget_cents = 0;
        self.extra_minutes = 0;
    }

    fn clear_conversation(&mut self) {
        self.messages.clear();
        self.question = None;
        self.asked = false;
        self.pending_hints.clear();
    }

    fn say(&mut self, from: Speaker, kind: MessageKind, text: String) {
        let step = self.completed;
        self.messages.push(RunMessage {
            from,
            kind,
            step,
            text,
        });
    }

    /// Bricht ab oder setzt zurück: Der Lauf ist danach wieder startbereit.
    pub fn reset(&mut self) {
        self.state = RunState::Idle;
        self.completed = 0;
        self.pr_number = None;
        self.plan_notes.clear();
        self.planned = false;
        self.clear_usage();
        self.clear_conversation();
    }

    /// Gibt den Plan frei: Ab jetzt arbeitet der Agent. `false`, wenn es nichts freizugeben gibt.
    pub fn approve(&mut self) -> bool {
        // Ein Plan, der verbotene Pfade berührt, lässt sich nicht freigeben.
        if !self.is_awaiting() || !self.violations().is_empty() {
            return false;
        }
        self.state = RunState::Running;
        self.say(Speaker::Du, MessageKind::Info, "Plan freigegeben.".into());
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
        self.say(
            Speaker::Du,
            MessageKind::Hint,
            format!("Anmerkung zum Plan: {note}"),
        );
        true
    }

    /// Beantwortet die offene Frage; danach arbeitet der Agent weiter. Leere Antworten
    /// zählen nicht.
    pub fn answer(&mut self, text: &str) -> bool {
        let text = text.trim();
        if !self.is_asking() || text.is_empty() {
            return false;
        }
        self.question = None;
        self.state = RunState::Running;
        self.say(Speaker::Du, MessageKind::Answer, text.to_string());
        true
    }

    /// Schickt dem Agenten einen Hinweis. Er berücksichtigt ihn beim nächsten Schritt.
    /// Möglich, solange der Lauf an den Schritten arbeitet (auch pausiert oder wartend).
    pub fn steer(&mut self, text: &str) -> bool {
        let text = text.trim();
        if !self.is_working() || text.is_empty() {
            return false;
        }
        self.pending_hints.push(text.to_string());
        self.say(Speaker::Du, MessageKind::Hint, text.to_string());
        true
    }

    /// Hält einen laufenden Agenten an. Eine offene Frage bleibt dabei bestehen.
    pub fn pause(&mut self) -> bool {
        if !self.is_running() {
            return false;
        }
        self.state = RunState::Paused;
        self.say(Speaker::Du, MessageKind::Info, "Lauf pausiert.".into());
        true
    }

    /// Setzt einen pausierten Lauf fort.
    pub fn resume(&mut self) -> bool {
        if !self.is_paused() {
            return false;
        }
        self.state = RunState::Running;
        self.say(Speaker::Du, MessageKind::Info, "Lauf fortgesetzt.".into());
        true
    }

    /// Setzt einen angehaltenen Lauf fort. `extra` erhöht, was ihn angehalten hat: Cent beim
    /// Budget des Laufs, Minuten beim Zeitlimit. Das Monatsbudget erhöht der Workspace selbst;
    /// hier ist es nur die Meldung. `false`, wenn der Lauf nicht angehalten ist.
    pub fn resume_after_stop(&mut self, extra: u64) -> bool {
        let Some(reason) = self.stop_reason() else {
            return false;
        };
        match reason {
            StopReason::RunBudget => {
                self.extra_budget_cents = self.extra_budget_cents.saturating_add(extra)
            }
            StopReason::Time => self.extra_minutes = self.extra_minutes.saturating_add(extra),
            StopReason::MonthBudget => {}
        }
        self.state = RunState::Running;
        let text = if extra == 0 {
            "Lauf fortgesetzt.".to_string()
        } else {
            format!(
                "Lauf fortgesetzt – {} um {} erhöht.",
                reason.raises(),
                reason.raise_label(extra)
            )
        };
        self.say(Speaker::Du, MessageKind::Info, text);
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

    /// Schließt den aktuellen Schritt ab. Gibt `true` zurück, wenn der Lauf damit
    /// fertig geworden ist.
    pub fn advance(&mut self) -> bool {
        self.tick() == Tick::Finished
    }

    /// Ein Takt: Der Agent berücksichtigt wartende Hinweise, stellt vielleicht eine
    /// Frage und schließt sonst den aktuellen Schritt ab.
    pub fn tick(&mut self) -> Tick {
        self.tick_with(&AgentLimits::unlimited(), 0)
    }

    /// Wie [`AgentRun::tick`], mit Grenzen. Ist vor dem Schritt eine Grenze erreicht, hält der Lauf
    /// an (`Stopped`) und tut nichts. `month_spent_cents` ist, was im Monat über alle Läufe
    /// ausgegeben wurde. Den Verbrauch des Schritts rechnet der Takt nach den Preisen der Grenzen aus.
    pub fn tick_with(&mut self, limits: &AgentLimits, month_spent_cents: u64) -> Tick {
        if !self.is_running() {
            return Tick::Idle;
        }
        if let Some(reason) = limits.reached(self, month_spent_cents) {
            self.state = RunState::Stopped(reason);
            let detail = limits.describe(reason, self, month_spent_cents);
            self.say(
                Speaker::Agent,
                MessageKind::Info,
                format!(
                    "Angehalten: {}. {detail} Erhöhe {} und setze den Lauf fort – oder brich ihn ab.",
                    reason.label(),
                    reason.raises()
                ),
            );
            return Tick::Stopped;
        }
        for hint in std::mem::take(&mut self.pending_hints) {
            self.say(
                Speaker::Agent,
                MessageKind::Ack,
                format!("Hinweis erhalten: {hint}"),
            );
        }
        // Die Rückfrage ist in der Demo fest verdrahtet (siehe `demo::agent_question`);
        // ein Backend würde sie vom echten Agenten bekommen.
        if !self.asked
            && let Some((step, question)) = crate::demo::agent_question(&self.ticket_key)
            && step == self.completed
        {
            self.asked = true;
            self.state = RunState::AwaitingAnswer;
            self.say(Speaker::Agent, MessageKind::Question, question.text.clone());
            self.question = Some(question);
            return Tick::Asked;
        }
        let done = self.steps()[self.completed.min(STEPS - 1)].clone();
        self.say(
            Speaker::Agent,
            MessageKind::Info,
            format!("{}: {}", done.title, done.text),
        );
        // Verbrauch des Schritts: in der Demo erfundene, aber feste Werte.
        let (input, output, millis) = crate::demo::step_tokens(&self.ticket_key, self.completed);
        self.record_usage(Usage::priced(input, output, millis, &limits.pricing));
        self.completed += 1;
        if self.completed == STEPS {
            self.state = RunState::Done;
            Tick::Finished
        } else {
            Tick::Stepped
        }
    }

    fn pr_label(&self) -> String {
        self.pr_number
            .map_or("Pull Request".to_string(), |n| format!("#{n}"))
    }

    /// Titel und Text jedes Schritts, passend zu den Optionen.
    pub fn steps(&self) -> Vec<Step> {
        let o = &self.options;
        let mut files = match self.files.len() {
            1 => "1 Datei angepasst".to_string(),
            n => format!("{n} Dateien angepasst"),
        };
        if self.rules.has_path_rules() {
            files.push_str(" · Pfad-Regeln eingehalten");
        }
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
        ];
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
                // Hat der Nutzer an diesem Schritt eine Frage beantwortet, steht es dabei.
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

    /// Protokoll bis einschließlich des Schritts, an dem der Agent gerade arbeitet.
    pub fn log(&self) -> String {
        let shown = if self.is_awaiting() {
            1
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
                1 => lines.push(format!("› git switch -c {}", self.branch())),
                2 => lines.push(format!("› Bearbeite {}", self.files.join(", "))),
                3 if !self.rules.required_tests.is_empty() => {
                    for cmd in &self.rules.required_tests {
                        lines.push(format!("› $ {cmd} … bestanden"));
                    }
                }
                3 if o.run_tests => lines.push("› Starte Lint und Tests … bestanden".into()),
                3 => lines.push("› Tests übersprungen".into()),
                _ => lines.push(format!(
                    "› Pull Request {} erstellt{}",
                    self.pr_label(),
                    if o.draft_pr { " (Entwurf)" } else { "" }
                )),
            }
            // Fragen, Antworten und berücksichtigte Hinweise dieses Schritts.
            for m in self.messages.iter().filter(|m| m.step == step) {
                match m.kind {
                    MessageKind::Question => lines.push(format!("› Frage: {}", m.text)),
                    MessageKind::Answer => lines.push(format!("› Antwort: {}", m.text)),
                    MessageKind::Ack => lines.push(format!("› {}", m.text)),
                    _ => {}
                }
            }
        }
        if self.is_asking() {
            lines.push("› Warte auf deine Antwort".into());
        } else if self.is_paused() {
            lines.push("› Pausiert".into());
        } else if let Some(reason) = self.stop_reason() {
            lines.push(format!("› Angehalten: {}", reason.label()));
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

    /// Ein Lauf ohne Rückfrage (die Demo fragt nur bei BAU-142).
    fn quiet() -> AgentRun {
        let mut r = run();
        r.ticket_key = "BAU-200".into();
        r
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
        let mut r = quiet();
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
        let mut r = quiet();
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

    /// Bringt BAU-142 bis zur Rückfrage.
    fn asking() -> AgentRun {
        let mut r = run();
        r.start(direct(), 92);
        assert_eq!(r.tick(), Tick::Stepped);
        assert_eq!(r.tick(), Tick::Stepped);
        assert_eq!(r.tick(), Tick::Asked);
        r
    }

    #[test]
    fn the_agent_asks_a_question_and_waits() {
        let mut r = asking();
        assert!(r.is_asking() && r.is_active() && !r.is_running());
        assert_eq!(r.completed, 2, "die Frage ist kein Schritt");
        let q = r.question.clone().unwrap();
        assert!(q.text.contains("deprecated"));
        assert_eq!(q.suggestions.len(), 3);
        assert_eq!(r.tick(), Tick::Idle, "ohne Antwort kein Fortschritt");
        assert!(!r.advance());
        assert_eq!(r.completed, 2);
        let steps = r.steps();
        assert_eq!(steps[2].title, "Frage an dich");
        assert_eq!(steps[2].state, StepState::Active);
        assert!(steps[3..].iter().all(|s| s.state == StepState::Pending));
        assert!(
            r.log().contains("› Frage: Soll der alte Stil"),
            "{}",
            r.log()
        );
        assert!(r.log().ends_with("› Warte auf deine Antwort"));
    }

    #[test]
    fn an_answer_continues_the_run_and_stays_in_the_history() {
        let mut r = asking();
        assert!(!r.answer("   "), "leer zählt nicht");
        assert!(r.is_asking());
        assert!(r.answer("  Als deprecated markieren "));
        assert!(r.is_running() && r.question.is_none());
        assert!(!r.answer("noch einmal"), "nur einmal antworten");
        let last = r.messages.last().unwrap();
        assert_eq!(
            (last.from, last.kind, last.text.as_str()),
            (Speaker::Du, MessageKind::Answer, "Als deprecated markieren")
        );
        assert!(r.log().contains("› Antwort: Als deprecated markieren"));
        // Die Frage kommt nur einmal; der Lauf geht zu Ende.
        while !r.advance() {}
        assert!(r.is_done());
        assert_eq!(
            r.messages
                .iter()
                .filter(|m| m.kind == MessageKind::Question)
                .count(),
            1
        );
        assert!(
            r.steps()[2]
                .text
                .ends_with("Deine Antwort: Als deprecated markieren")
        );
    }

    #[test]
    fn other_tickets_ask_nothing() {
        let mut r = quiet();
        r.start(direct(), 92);
        while !r.advance() {}
        assert!(r.messages.iter().all(|m| m.kind != MessageKind::Question));
    }

    #[test]
    fn a_hint_is_taken_into_account_at_the_next_step() {
        let mut r = quiet();
        r.start(direct(), 92);
        assert!(!r.steer("  "), "leer zählt nicht");
        assert!(r.steer(" Auch den Slider prüfen "));
        assert_eq!(r.pending_hints, ["Auch den Slider prüfen"]);
        assert!(
            !r.messages.iter().any(|m| m.kind == MessageKind::Ack),
            "noch nicht berücksichtigt"
        );
        assert_eq!(r.tick(), Tick::Stepped);
        assert!(r.pending_hints.is_empty());
        let ack = r
            .messages
            .iter()
            .find(|m| m.kind == MessageKind::Ack)
            .unwrap();
        assert_eq!(
            (ack.from, ack.text.as_str()),
            (Speaker::Agent, "Hinweis erhalten: Auch den Slider prüfen")
        );
        assert!(
            r.log()
                .contains("› Hinweis erhalten: Auch den Slider prüfen")
        );
    }

    #[test]
    fn hints_only_work_while_the_agent_works() {
        let mut r = quiet();
        assert!(!r.steer("zu früh"), "idle");
        r.start(AgentOptions::default(), 92);
        assert!(
            !r.steer("Plan noch offen"),
            "dafür gibt es die Plan-Anmerkung"
        );
        r.approve();
        assert!(r.steer("jetzt"));
        while !r.advance() {}
        assert!(!r.steer("zu spät"), "fertig");
    }

    #[test]
    fn pause_and_resume() {
        let mut r = quiet();
        assert!(!r.pause() && !r.resume());
        r.start(direct(), 92);
        r.advance();
        assert!(r.pause() && r.is_paused() && r.is_active());
        assert!(!r.pause(), "schon pausiert");
        assert_eq!(r.tick(), Tick::Idle);
        assert_eq!(r.completed, 1, "pausiert heißt: kein Fortschritt");
        assert_eq!(r.steps()[1].state, StepState::Active);
        assert!(r.steps()[1].text.starts_with("Pausiert"));
        assert!(r.log().ends_with("› Pausiert"));
        // Hinweise lassen sich auch pausiert schicken und warten bis zur Fortsetzung.
        assert!(r.steer("Während der Pause"));
        assert_eq!(r.tick(), Tick::Idle);
        assert_eq!(r.pending_hints.len(), 1);
        assert!(r.resume() && r.is_running() && !r.resume());
        assert_eq!(r.tick(), Tick::Stepped);
        assert!(r.pending_hints.is_empty());
    }

    #[test]
    fn a_question_cannot_be_skipped_by_pausing() {
        let mut r = asking();
        assert!(!r.pause(), "wer fragt, läuft nicht");
        assert!(r.is_asking());
    }

    #[test]
    fn reset_clears_the_conversation() {
        let mut r = asking();
        r.steer("noch ein Hinweis");
        r.reset();
        assert!(r.messages.is_empty() && r.question.is_none() && r.pending_hints.is_empty());
        // Ein neuer Lauf fragt wieder.
        r.start(direct(), 93);
        assert_eq!(r.messages.len(), 1);
        r.advance();
        r.advance();
        assert_eq!(r.tick(), Tick::Asked);
    }

    #[test]
    fn the_history_tells_the_story() {
        let mut r = run();
        r.start(
            AgentOptions {
                hint: "Bitte vorsichtig".into(),
                ..AgentOptions::default()
            },
            92,
        );
        r.revise("Auch Tests");
        r.approve();
        let texts: Vec<_> = r.messages.iter().map(|m| (m.from, m.kind)).collect();
        assert_eq!(
            texts,
            [
                (Speaker::Agent, MessageKind::Info),
                (Speaker::Du, MessageKind::Hint),
                (Speaker::Agent, MessageKind::Info),
                (Speaker::Du, MessageKind::Hint),
                (Speaker::Du, MessageKind::Info),
            ]
        );
    }
}
