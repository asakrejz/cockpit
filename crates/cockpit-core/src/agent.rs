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
//!
//! Jeder Schritt hat einen Verbrauch ([`AgentRun::step_usage`]). Erreicht ein Lauf eine Grenze
//! ([`AgentLimits`]), hält er an ([`RunState::Stopped`]) und wartet darauf, dass du das Limit
//! erhöhst und ihn fortsetzt – oder abbrichst. Die Regeln des Repos ([`RepoRules`]) gelten ab dem
//! Start: Sie stehen im Plan, und ein Plan, der verbotene Pfade berührt, lässt sich nicht freigeben.

use serde::{Deserialize, Serialize};

use crate::diagnosis::Diagnosis;
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
    /// Die Diagnose eines roten Checks liegt vor; der Agent wartet, ob er den Fix übernehmen soll.
    AwaitingFix,
    /// Eine Grenze (Budget, Zeitlimit) ist erreicht: Der Lauf wartet auf dich.
    Stopped(StopReason),
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

/// Der Plan, den der echte Agent aus dem Container vorgelegt hat (statt des abgeleiteten).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentPlan {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
}

/// Was der echte Agent aus dem Container meldet. Das Backend prüft die Zeilen des Containers und
/// reicht sie als `Action::ReportAgent` (Urheber Agent) an; `cockpit-core` entscheidet, was sie
/// bewirken – der Agent kann den Zustand nur auf diesen Wegen ändern.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AgentReport {
    /// Der Container läuft. `detail` ist ein Satz für den Verlauf (Laufzeit, Modus).
    Started { detail: String },
    /// Der Plan liegt vor (nur, solange der Lauf auf die Freigabe wartet).
    Plan {
        summary: String,
        files: Vec<String>,
        steps: Vec<String>,
    },
    /// Ein Arbeitsschritt mit seinem Verbrauch.
    Step {
        title: String,
        text: String,
        input_tokens: u64,
        output_tokens: u64,
        millis: u64,
    },
    /// Die Dateien, die der Agent geändert hat.
    Files { files: Vec<String> },
    /// Eine Rückfrage; der Lauf wartet auf die Antwort.
    Question {
        text: String,
        #[serde(default)]
        suggestions: Vec<String>,
    },
    /// Die Hinweise des Nutzers sind beim Agenten angekommen.
    HintsDelivered,
    /// Fertig: Der Branch ist gepusht.
    Done { summary: String },
    /// Der Lauf endete ohne Ergebnis (Fehler, Zeitüberschreitung, Neustart). `reason` in Klartext.
    Failed { reason: String },
}

/// Was eine Meldung des Agenten bewirkt hat; der Workspace verbucht und protokolliert es.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportEffect {
    /// Sätze für das Protokoll.
    pub notes: Vec<String>,
    /// Neuer Verbrauch für das Kassenbuch.
    pub usage: Option<Usage>,
    /// Der Lauf ist fertig geworden.
    pub finished: bool,
    /// Der Lauf wurde beendet und startbereit zurückgesetzt.
    pub failed: bool,
    /// Eine Grenze hat den Lauf angehalten.
    pub stopped: Option<StopReason>,
    /// Der Lauf wartet jetzt auf eine Antwort.
    pub asking: bool,
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
    /// Was der Lauf tut. Alte Stände kennen nur „Ticket umsetzen“.
    #[serde(default)]
    pub kind: RunKind,
    /// Branch des Pull Requests, auf den ein Folgeauftrag pusht. Leer bei „Ticket umsetzen“.
    #[serde(default)]
    pub target_branch: String,
    /// Die Diagnose einer Untersuchung, sobald sie vorliegt.
    #[serde(default)]
    pub diagnosis: Option<Diagnosis>,
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
    /// Der Lauf gehört dem echten Agenten (Container); der simulierte Takt fasst ihn nicht an.
    #[serde(default)]
    pub real: bool,
    /// Der Plan aus dem Container. Solange er fehlt, arbeitet der Agent noch daran.
    #[serde(default)]
    pub agent_plan: Option<AgentPlan>,
    /// Warum der letzte Lauf ohne Ergebnis endete (Klartext); leer, solange alles gut ging.
    #[serde(default)]
    pub ended: Option<String>,
    /// Das Ergebnis des Agenten, wenn er fertig ist.
    #[serde(default)]
    pub summary: String,
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
            repo: String::new(),
            rules: RepoRules::default(),
            step_usage: Vec::new(),
            extra_budget_cents: 0,
            extra_minutes: 0,
            real: false,
            agent_plan: None,
            ended: None,
            summary: String::new(),
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
            || self.is_diagnosed()
            || self.is_stopped()
    }

    /// Der Agent arbeitet an den Schritten (läuft, fragt, ist pausiert, angehalten oder wartet
    /// auf die Entscheidung über seinen Fix).
    fn is_working(&self) -> bool {
        self.is_running()
            || self.is_asking()
            || self.is_paused()
            || self.is_diagnosed()
            || self.is_stopped()
    }

    /// Die Diagnose liegt vor; der Agent wartet auf „Fix übernehmen“ oder „Verwerfen“.
    pub fn is_diagnosed(&self) -> bool {
        self.state == RunState::AwaitingFix
    }

    /// Wartet der Lauf auf eine Entscheidung von dir (Plan, Antwort oder Fix)?
    pub fn is_waiting_for_you(&self) -> bool {
        self.is_awaiting() || self.is_asking() || self.is_diagnosed()
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

    /// `claude/BAU-142-toggle-fokus-ring-fehlt`
    pub fn branch(&self) -> String {
        if !self.target_branch.is_empty() {
            return self.target_branch.clone();
        }
        format!(
            "claude/{}-{}",
            self.ticket_key,
            slug(&self.ticket_title, 24)
        )
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
        self.agent_plan = None;
        self.ended = None;
        self.summary.clear();
        self.clear_usage();
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
        self.pr_number = self.kind.review();
        self.plan_notes.clear();
        self.planned = false;
        self.diagnosis = None;
        self.agent_plan = None;
        self.summary.clear();
        self.real = false;
        self.clear_usage();
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
        // Ein Plan, der verbotene Pfade berührt, lässt sich nicht freigeben.
        if !self.is_awaiting() || !self.violations().is_empty() || self.is_drafting() {
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
        // Der echte Agent erarbeitet einen neuen Plan; bis er vorliegt, ist keine Freigabe möglich.
        if self.real {
            self.agent_plan = None;
        }
        true
    }

    /// Der echte Agent erarbeitet den Plan noch: Es gibt nichts freizugeben.
    pub fn is_drafting(&self) -> bool {
        self.is_awaiting() && self.real && self.agent_plan.is_none()
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

    /// Nimmt eine Meldung des echten Agenten entgegen. `None`, wenn sie im jetzigen Zustand nicht
    /// passt (der Lauf ist nicht aktiv, der Plan wird nicht erwartet …): Dann ändert sich nichts.
    /// Die Meldungen sind die einzige Möglichkeit, wie der Container den Lauf verändert.
    pub fn report(
        &mut self,
        report: AgentReport,
        limits: &AgentLimits,
        month_spent_cents: u64,
    ) -> Option<ReportEffect> {
        if !self.is_active() {
            return None;
        }
        let mut effect = ReportEffect::default();
        match report {
            AgentReport::Started { detail } => {
                self.real = true;
                effect
                    .notes
                    .push(format!("{}: Container gestartet", self.label()));
                self.say(Speaker::Agent, MessageKind::Info, detail);
            }
            AgentReport::Plan {
                summary,
                files,
                steps,
            } => {
                if !self.is_awaiting() {
                    return None;
                }
                self.real = true;
                self.files = files.clone();
                self.agent_plan = Some(AgentPlan {
                    summary,
                    files,
                    steps,
                });
                self.say(
                    Speaker::Agent,
                    MessageKind::Info,
                    "Der Plan liegt vor. Ich warte auf deine Freigabe.".into(),
                );
                effect.notes.push(format!(
                    "Plan für {} vorgelegt, wartet auf Freigabe",
                    self.label()
                ));
            }
            AgentReport::Step {
                title,
                text,
                input_tokens,
                output_tokens,
                millis,
            } => {
                if !(self.is_working() || self.is_awaiting()) {
                    return None;
                }
                let usage = Usage::priced(input_tokens, output_tokens, millis, &limits.pricing);
                self.record_usage(usage);
                effect.usage = Some(usage);
                let line = if text.trim().is_empty() {
                    title.clone()
                } else {
                    format!("{title}: {text}")
                };
                self.say(Speaker::Agent, MessageKind::Info, line);
                if self.is_working() {
                    // Der letzte Schritt gehört dem Ergebnis; nur `Done` schließt den Lauf ab.
                    if self.completed + 1 < STEPS {
                        self.completed += 1;
                    }
                    effect.notes.push(format!("{}: {title}", self.label()));
                    if self.is_running()
                        && let Some(reason) =
                            limits.reached(self, month_spent_cents.saturating_add(usage.cost_cents))
                    {
                        self.state = RunState::Stopped(reason);
                        let detail = limits.describe(
                            reason,
                            self,
                            month_spent_cents.saturating_add(usage.cost_cents),
                        );
                        self.say(
                            Speaker::Agent,
                            MessageKind::Info,
                            format!(
                                "Angehalten: {}. {detail} Erhöhe {} und setze den Lauf fort – oder brich ihn ab.",
                                reason.label(),
                                reason.raises()
                            ),
                        );
                        effect.notes.push(format!(
                            "{}: Angehalten – {}",
                            self.label(),
                            reason.label()
                        ));
                        effect.stopped = Some(reason);
                    }
                }
            }
            AgentReport::Files { files } => {
                if !self.is_working() {
                    return None;
                }
                self.files = files;
            }
            AgentReport::Question { text, suggestions } => {
                if !self.is_running() || text.trim().is_empty() {
                    return None;
                }
                self.asked = true;
                self.state = RunState::AwaitingAnswer;
                self.say(Speaker::Agent, MessageKind::Question, text.clone());
                self.question = Some(Question { text, suggestions });
                effect.asking = true;
                effect.notes.push(format!(
                    "{}: Der Agent hat eine Frage und wartet auf deine Antwort",
                    self.label()
                ));
            }
            AgentReport::HintsDelivered => {
                for hint in std::mem::take(&mut self.pending_hints) {
                    self.say(
                        Speaker::Agent,
                        MessageKind::Ack,
                        format!("Hinweis erhalten: {hint}"),
                    );
                }
            }
            AgentReport::Done { summary } => {
                if !self.is_working() {
                    return None;
                }
                self.state = RunState::Done;
                self.completed = STEPS;
                self.summary = summary.clone();
                self.question = None;
                self.say(Speaker::Agent, MessageKind::Info, summary);
                effect.finished = true;
            }
            AgentReport::Failed { reason } => {
                let label = self.label();
                self.reset();
                self.ended = Some(reason.clone());
                self.say(
                    Speaker::Agent,
                    MessageKind::Info,
                    format!("Der Lauf endete ohne Ergebnis: {reason}"),
                );
                effect.failed = true;
                effect
                    .notes
                    .push(format!("{label}: Lauf beendet – {reason}"));
            }
        }
        Some(effect)
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
        if !self.is_running() || self.real {
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
        // Verbrauch des Schritts: in der Demo erfundene, aber feste Werte.
        let (input, output, millis) = crate::demo::step_tokens(&self.ticket_key, self.completed);
        self.record_usage(Usage::priced(input, output, millis, &limits.pricing));
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

    fn build_steps(&self, texts: [(&'static str, String); STEPS]) -> Vec<Step> {
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

    /// Das Protokoll eines echten Laufs: was der Agent und du gesagt haben, dazu der Verbrauch.
    fn real_log(&self) -> String {
        let mut lines: Vec<String> = self
            .messages
            .iter()
            .map(|m| {
                let wer = match m.from {
                    Speaker::Agent => "Agent",
                    Speaker::Du => "Du",
                };
                format!("› {wer}: {}", m.text)
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
        } else if let Some(reason) = self.stop_reason() {
            lines.push(format!("› Angehalten: {}", reason.label()));
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
        assert_eq!(r.branch(), "claude/BAU-142-toggle-fokus-ring-fehlt");
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
        assert!(plan.steps.iter().any(|s| s.contains("claude/BAU-142")));
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

    fn real_run() -> AgentRun {
        let mut r = run();
        r.start(
            AgentOptions {
                plan_first: false,
                ..AgentOptions::default()
            },
            92,
        );
        r.report(
            AgentReport::Started {
                detail: "Container gestartet".into(),
            },
            &AgentLimits::unlimited(),
            0,
        );
        r
    }

    fn step(tokens_in: u64, tokens_out: u64, millis: u64) -> AgentReport {
        AgentReport::Step {
            title: "Datei lesen".into(),
            text: "src/lib.rs".into(),
            input_tokens: tokens_in,
            output_tokens: tokens_out,
            millis,
        }
    }

    #[test]
    fn a_real_run_is_not_touched_by_the_simulated_tick() {
        let mut r = real_run();
        assert!(r.real);
        assert_eq!(r.tick(), Tick::Idle);
        assert_eq!(r.completed, 0);
    }

    #[test]
    fn steps_record_usage_with_the_prices_and_never_finish_the_run() {
        let mut r = real_run();
        let limits = AgentLimits::unlimited();
        for _ in 0..10 {
            let e = r.report(step(1_000_000, 0, 1_000), &limits, 0).unwrap();
            assert_eq!(e.usage.unwrap().cost_cents, 300);
            assert!(!e.finished);
        }
        assert_eq!(r.completed, STEPS - 1);
        assert!(r.is_running());
        assert_eq!(r.usage().cost_cents, 3_000);
        assert_eq!(r.usage().millis, 10_000);
    }

    #[test]
    fn the_budget_stops_a_real_run_with_a_plain_message() {
        let mut r = real_run();
        let limits = AgentLimits {
            run_budget_cents: Some(100),
            ..AgentLimits::unlimited()
        };
        let e = r.report(step(1_000_000, 0, 10), &limits, 0).unwrap();
        assert_eq!(e.stopped, Some(StopReason::RunBudget));
        assert_eq!(r.stop_reason(), Some(StopReason::RunBudget));
        assert!(
            r.messages
                .last()
                .unwrap()
                .text
                .starts_with("Angehalten: Budget pro Lauf erreicht")
        );
    }

    #[test]
    fn the_month_budget_counts_what_the_step_just_cost() {
        let mut r = real_run();
        let limits = AgentLimits {
            month_budget_cents: Some(400),
            ..AgentLimits::unlimited()
        };
        let e = r.report(step(1_000_000, 0, 10), &limits, 150).unwrap();
        assert_eq!(e.stopped, Some(StopReason::MonthBudget));
    }

    #[test]
    fn a_question_waits_for_the_answer_and_the_answer_resumes() {
        let mut r = real_run();
        let limits = AgentLimits::unlimited();
        let e = r
            .report(
                AgentReport::Question {
                    text: "Welcher Branch?".into(),
                    suggestions: vec!["main".into()],
                },
                &limits,
                0,
            )
            .unwrap();
        assert!(e.asking && r.is_asking());
        assert_eq!(r.question.as_ref().unwrap().suggestions, ["main"]);
        assert!(r.answer("main"));
        assert!(r.is_running());
    }

    #[test]
    fn hints_are_acknowledged_when_the_container_got_them() {
        let mut r = real_run();
        r.steer("Bitte kleine Commits");
        assert_eq!(r.pending_hints.len(), 1);
        r.report(AgentReport::HintsDelivered, &AgentLimits::unlimited(), 0);
        assert!(r.pending_hints.is_empty());
        assert_eq!(r.messages.last().unwrap().kind, MessageKind::Ack);
    }

    #[test]
    fn done_finishes_and_failed_resets_with_the_reason() {
        let mut r = real_run();
        let limits = AgentLimits::unlimited();
        let e = r
            .report(
                AgentReport::Done {
                    summary: "Fertig".into(),
                },
                &limits,
                0,
            )
            .unwrap();
        assert!(e.finished && r.is_done() && r.completed == STEPS);

        let mut r = real_run();
        let e = r
            .report(
                AgentReport::Failed {
                    reason: "Der Container endete mit Code 1".into(),
                },
                &limits,
                0,
            )
            .unwrap();
        assert!(e.failed && r.is_idle() && !r.real);
        assert_eq!(r.ended.as_deref(), Some("Der Container endete mit Code 1"));
        assert!(r.log().contains("Der Container endete mit Code 1"));
        // Ein neuer Start löscht den Grund.
        r.start(AgentOptions::default(), 92);
        assert_eq!(r.ended, None);
    }

    #[test]
    fn reports_for_an_inactive_run_change_nothing() {
        let mut r = run();
        let before = r.clone();
        assert!(
            r.report(step(1, 1, 1), &AgentLimits::unlimited(), 0)
                .is_none()
        );
        assert_eq!(r, before);
    }

    #[test]
    fn the_plan_comes_from_the_agent_and_cannot_be_approved_before() {
        let mut r = run();
        let limits = AgentLimits::unlimited();
        r.start(AgentOptions::default(), 92);
        r.report(
            AgentReport::Started {
                detail: "Plan-Lauf".into(),
            },
            &limits,
            0,
        );
        assert!(r.is_drafting());
        assert!(!r.approve(), "ohne Plan gibt es nichts freizugeben");
        r.report(
            AgentReport::Plan {
                summary: "Fokus-Ring ergänzen".into(),
                files: vec!["src/toggle.rs".into()],
                steps: vec!["Lesen".into(), "Ändern".into()],
            },
            &limits,
            0,
        );
        let plan = r.plan().unwrap();
        assert_eq!(plan.summary, "Fokus-Ring ergänzen");
        assert_eq!(r.files, ["src/toggle.rs"]);
        assert!(!r.is_drafting());
        // Eine Anmerkung verlangt einen neuen Plan.
        assert!(r.revise("Auch Tests"));
        assert!(r.is_drafting() && !r.approve());
        r.report(
            AgentReport::Plan {
                summary: "Neu".into(),
                files: vec![],
                steps: vec![],
            },
            &limits,
            0,
        );
        assert!(r.approve());
    }

    #[test]
    fn a_plan_report_outside_the_approval_is_ignored() {
        let mut r = real_run();
        assert!(
            r.report(
                AgentReport::Plan {
                    summary: "x".into(),
                    files: vec![],
                    steps: vec![]
                },
                &AgentLimits::unlimited(),
                0
            )
            .is_none()
        );
    }
}
