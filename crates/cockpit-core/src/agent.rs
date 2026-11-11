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
//! Neben dem Lauf „Ticket umsetzen“ gibt es zwei Folgeaufträge ([`RunKind`]): Review-Kommentare
//! umsetzen (neuer Commit auf demselben Branch, kein neuer Pull Request) und einen roten Check
//! untersuchen. Die Untersuchung hält nach der [`Diagnosis`] an ([`RunState::AwaitingFix`]) und
//! tut nichts, bevor du den Fix übernimmst.

use serde::{Deserialize, Serialize};

use crate::diagnosis::Diagnosis;
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
    /// Der Agent hat eine Frage gestellt und wartet auf die Antwort.
    AwaitingAnswer,
    /// Der Nutzer hat den Lauf angehalten.
    Paused,
    /// Die Diagnose eines roten Checks liegt vor; der Agent wartet, ob er den Fix übernehmen soll.
    AwaitingFix,
    Done,
}

/// Was der Lauf tut.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub enum RunKind {
    /// Ein Ticket umsetzen und einen Pull Request anlegen.
    #[default]
    Implement,
    /// Kommentare eines Reviews umsetzen und auf den Branch des Pull Requests pushen.
    AddressReview {
        /// Nummer des Pull Requests.
        review: u32,
        /// IDs der Zeilenkommentare, die der Agent umsetzen soll.
        comments: Vec<String>,
    },
    /// Einen roten Check untersuchen, eine Diagnose vorlegen und – nach deiner Freigabe –
    /// den Fix pushen.
    Diagnose {
        /// Nummer des Pull Requests.
        review: u32,
        /// Name des Checks.
        check: String,
    },
}

impl RunKind {
    /// Der Pull Request, auf dem ein Folgeauftrag arbeitet.
    pub fn review(&self) -> Option<u32> {
        match self {
            RunKind::Implement => None,
            RunKind::AddressReview { review, .. } | RunKind::Diagnose { review, .. } => {
                Some(*review)
            }
        }
    }
}

/// Nach so vielen abgeschlossenen Schritten legt die Untersuchung die Diagnose vor.
const DIAGNOSED_AT: usize = 3;

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
    /// Die Untersuchung hat die Diagnose vorgelegt und wartet auf die Entscheidung.
    Diagnosed,
    /// Ein Schritt wurde abgeschlossen.
    Stepped,
    /// Der letzte Schritt wurde abgeschlossen.
    Finished,
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
    /// Was der Lauf tut. Alte Stände kennen nur „Ticket umsetzen“.
    #[serde(default)]
    pub kind: RunKind,
    /// Branch des Pull Requests, auf den ein Folgeauftrag pusht. Leer bei „Ticket umsetzen“.
    #[serde(default)]
    pub target_branch: String,
    /// Die Diagnose einer Untersuchung, sobald sie vorliegt.
    #[serde(default)]
    pub diagnosis: Option<Diagnosis>,
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
            kind: RunKind::Implement,
            target_branch: String::new(),
            diagnosis: None,
        }
    }

    /// Ein Folgeauftrag auf einem bestehenden Pull Request. `item_key` ist der Eintrag, an dem
    /// der Pull Request hängt, `branch` sein Branch.
    pub fn followup(kind: RunKind, item_key: &str, title: &str, branch: &str) -> Self {
        let mut run = Self::new(item_key, title);
        run.pr_number = kind.review();
        run.kind = kind;
        run.target_branch = branch.into();
        run
    }

    /// Der Schlüssel des Laufs, unter dem Seiten, Aktionen und Protokoll ihn finden:
    /// bei „Ticket umsetzen“ der Ticket-Schlüssel, sonst `review-91` bzw. `check-85-lint`.
    pub fn key(&self) -> String {
        match &self.kind {
            RunKind::Implement => self.ticket_key.clone(),
            RunKind::AddressReview { review, .. } => format!("review-{review}"),
            RunKind::Diagnose { review, check } => {
                format!("check-{review}-{}", slug(check, 24))
            }
        }
    }

    /// Kurzer Name für Meldungen: der Schlüssel, bei Folgeaufträgen mit Klartext.
    pub fn label(&self) -> String {
        match &self.kind {
            RunKind::Implement => self.ticket_key.clone(),
            RunKind::AddressReview { review, .. } => format!("Kommentare in #{review}"),
            RunKind::Diagnose { review, check } => format!("Check „{check}“ in #{review}"),
        }
    }

    pub fn is_followup(&self) -> bool {
        self.kind != RunKind::Implement
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

    /// Wartet, fragt, pausiert oder läuft – der Lauf belegt das Ticket.
    pub fn is_active(&self) -> bool {
        self.is_awaiting()
            || self.is_running()
            || self.is_asking()
            || self.is_paused()
            || self.is_diagnosed()
    }

    /// Der Agent arbeitet an den Schritten (läuft, fragt, ist pausiert oder wartet auf die
    /// Entscheidung über seinen Fix).
    fn is_working(&self) -> bool {
        self.is_running() || self.is_asking() || self.is_paused() || self.is_diagnosed()
    }

    /// Die Diagnose liegt vor; der Agent wartet auf „Fix übernehmen“ oder „Verwerfen“.
    pub fn is_diagnosed(&self) -> bool {
        self.state == RunState::AwaitingFix
    }

    /// Wartet der Lauf auf eine Entscheidung von dir (Plan, Antwort oder Fix)?
    pub fn is_waiting_for_you(&self) -> bool {
        self.is_awaiting() || self.is_asking() || self.is_diagnosed()
    }

    pub fn is_running(&self) -> bool {
        self.state == RunState::Running
    }

    pub fn is_done(&self) -> bool {
        self.state == RunState::Done
    }

    /// `agent/BAU-142-toggle-fokus-ring-fehlt`
    pub fn branch(&self) -> String {
        if !self.target_branch.is_empty() {
            return self.target_branch.clone();
        }
        format!("agent/{}-{}", self.ticket_key, slug(&self.ticket_title, 24))
    }

    pub fn start(&mut self, options: AgentOptions, pr_number: u32) {
        // Eine Untersuchung ändert erst nach deiner Freigabe des Fixes etwas, ein Plan wäre
        // doppelt. Folgeaufträge legen keinen neuen Pull Request an.
        let investigate = matches!(self.kind, RunKind::Diagnose { .. });
        self.planned = options.plan_first && !investigate;
        self.state = if self.planned {
            RunState::AwaitingApproval
        } else {
            RunState::Running
        };
        self.options = options;
        self.completed = 0;
        self.pr_number = Some(self.kind.review().unwrap_or(pr_number));
        self.plan_notes.clear();
        self.diagnosis = None;
        self.clear_conversation();
        self.say(
            Speaker::Agent,
            MessageKind::Info,
            format!("Auftrag für {} angenommen.", self.label()),
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
        self.pr_number = self.kind.review();
        self.plan_notes.clear();
        self.planned = false;
        self.diagnosis = None;
        self.clear_conversation();
    }

    /// Übernimmt den Fix der Diagnose: Der Agent wendet ihn an, pusht und startet den Check
    /// neu. `false`, wenn keine Diagnose wartet.
    pub fn accept_fix(&mut self) -> bool {
        if !self.is_diagnosed() {
            return false;
        }
        self.state = RunState::Running;
        self.say(Speaker::Du, MessageKind::Info, "Fix übernommen.".into());
        true
    }

    /// Gibt den Plan frei: Ab jetzt arbeitet der Agent. `false`, wenn es nichts freizugeben gibt.
    pub fn approve(&mut self) -> bool {
        if !self.is_awaiting() {
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

    /// Der aktuelle Plan. Vor dem Start gibt es keinen.
    pub fn plan(&self) -> Option<Plan> {
        if !self.planned {
            return None;
        }
        if let RunKind::AddressReview { review, comments } = &self.kind {
            return Some(self.address_plan(*review, comments.len()));
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

    /// Plan für „Review-Kommentare umsetzen“: kein neuer Branch, kein neuer Pull Request.
    fn address_plan(&self, review: u32, comments: usize) -> Plan {
        let o = &self.options;
        let hint = o.hint.trim();
        let branch = self.branch();
        let mut summary = format!(
            "{} aus Review #{review} umsetzen: {} ändern{}, auf {branch} pushen.",
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
            format!("{} aus Review #{review} lesen", plural_comments(comments)),
            format!("Branch {branch} auschecken"),
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
            "Auf {branch} pushen – kein neuer Pull Request, kein Merge"
        ));
        if !self.plan_notes.is_empty() {
            summary.push_str(&format!(" Überarbeitet ({}×).", self.plan_notes.len()));
        }
        Plan {
            summary,
            files: self.files.clone(),
            steps,
        }
    }

    /// Schließt den aktuellen Schritt ab. Gibt `true` zurück, wenn der Lauf damit
    /// fertig geworden ist.
    pub fn advance(&mut self) -> bool {
        self.tick() == Tick::Finished
    }

    /// Ein Takt: Der Agent berücksichtigt wartende Hinweise, stellt vielleicht eine
    /// Frage und schließt sonst den aktuellen Schritt ab.
    pub fn tick(&mut self) -> Tick {
        if !self.is_running() {
            return Tick::Idle;
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
            && let Some((step, question)) = crate::demo::agent_question(&self.key())
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
        self.completed += 1;
        if self.completed == STEPS {
            self.state = RunState::Done;
            Tick::Finished
        } else if self.completed == DIAGNOSED_AT && matches!(self.kind, RunKind::Diagnose { .. }) {
            // Die Diagnose liegt vor. Bis zu deiner Entscheidung bleibt der Branch unberührt.
            self.state = RunState::AwaitingFix;
            self.say(
                Speaker::Agent,
                MessageKind::Info,
                "Die Diagnose liegt vor. Ich warte, ob ich den Fix übernehmen soll.".into(),
            );
            Tick::Diagnosed
        } else {
            Tick::Stepped
        }
    }

    fn pr_label(&self) -> String {
        self.pr_number
            .map_or("Pull Request".to_string(), |n| format!("#{n}"))
    }

    /// Titel und Text jedes Schritts, passend zu Art und Optionen des Laufs.
    pub fn steps(&self) -> Vec<Step> {
        let texts = match &self.kind {
            RunKind::Implement => self.implement_texts(),
            RunKind::AddressReview { review, comments } => {
                self.address_texts(*review, comments.len())
            }
            RunKind::Diagnose { review, check } => self.diagnose_texts(*review, check),
        };
        self.build_steps(texts)
    }

    fn address_texts(&self, review: u32, comments: usize) -> [(&'static str, String); STEPS] {
        let o = &self.options;
        [
            (
                "Kommentare gelesen",
                format!(
                    "{} aus Review #{review} ausgewertet",
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

    fn diagnose_texts(&self, review: u32, check: &str) -> [(&'static str, String); STEPS] {
        let code = self.diagnosis.as_ref().is_none_or(Diagnosis::changes_code);
        [
            (
                "Log gelesen",
                format!("Check „{check}“ von #{review}: Ausgabe ausgewertet"),
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

    fn implement_texts(&self) -> [(&'static str, String); STEPS] {
        let o = &self.options;
        let files = match self.files.len() {
            1 => "1 Datei angepasst".to_string(),
            n => format!("{n} Dateien angepasst"),
        };
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
        ]
    }

    fn build_steps(&self, texts: [(&'static str, String); STEPS]) -> Vec<Step> {
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
                if i == self.completed && self.is_diagnosed() {
                    return Step {
                        title: "Diagnose liegt vor".to_string(),
                        text: "Wartet auf deine Entscheidung – bis dahin bleibt der Branch unverändert"
                            .to_string(),
                        state: StepState::Active,
                    };
                }
                if i == self.completed && self.is_asking() {
                    return Step {
                        title: "Frage an dich".to_string(),
                        text: "Wartet auf deine Antwort".to_string(),
                        state: StepState::Active,
                    };
                }
                if i == self.completed && self.is_paused() {
                    return Step {
                        title: title.to_string(),
                        text: "Pausiert – setze den Lauf fort, damit es weitergeht".to_string(),
                        state: StepState::Active,
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

    /// Protokollzeilen eines Schritts von „Review-Kommentare umsetzen“.
    fn address_log(&self, lines: &mut Vec<String>, step: usize, review: u32, comments: usize) {
        let o = &self.options;
        let hint = o.hint.trim();
        match step {
            0 => {
                lines.push(format!(
                    "› Lese {} aus Review #{review}",
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

    /// Protokollzeilen eines Schritts von „Check untersuchen“.
    fn diagnose_log(&self, lines: &mut Vec<String>, step: usize, review: u32, check: &str) {
        let code = self.diagnosis.as_ref().is_none_or(Diagnosis::changes_code);
        match step {
            0 => lines.push(format!("› Lese das Log von „{check}“ in #{review}")),
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

    /// Protokoll bis einschließlich des Schritts, an dem der Agent gerade arbeitet.
    pub fn log(&self) -> String {
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
                    self.address_log(&mut lines, step, *review, comments.len());
                }
                (RunKind::Diagnose { review, check }, _) => {
                    self.diagnose_log(&mut lines, step, *review, check);
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
                (RunKind::Implement, 1) => lines.push(format!("› git switch -c {}", self.branch())),
                (RunKind::Implement, 2) => {
                    lines.push(format!("› Bearbeite {}", self.files.join(", ")))
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
        if self.is_diagnosed() {
            lines.push("› Warte auf deine Entscheidung über den Fix".into());
        } else if self.is_asking() {
            lines.push("› Warte auf deine Antwort".into());
        } else if self.is_paused() {
            lines.push("› Pausiert".into());
        }
        lines.join("\n")
    }
}

fn plural_comments(n: usize) -> String {
    match n {
        1 => "1 Kommentar".to_string(),
        n => format!("{n} Kommentare"),
    }
}

fn files_label(n: usize) -> String {
    match n {
        1 => "1 Datei".to_string(),
        n => format!("{n} Dateien"),
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
