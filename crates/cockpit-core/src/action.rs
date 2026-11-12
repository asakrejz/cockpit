//! Aktionen und Protokoll.
//!
//! Jede Änderung, die die Oberfläche auslösen kann, ist eine [`Action`]. Wer sie
//! anwendet – im Demo-Modus der Browser, später das Backend –, ruft
//! [`Workspace::apply`](crate::Workspace::apply) auf und bekommt entweder ein
//! [`Applied`] oder einen [`ActionError`]. Jede erfolgreiche Aktion hinterlässt
//! einen [`LogEntry`] im Protokoll des Workspace; Fehlschläge hinterlassen nichts.
//!
//! Aktionen sind `serde`-fähig, damit das Backend sie als JSON empfangen kann.

use serde::{Deserialize, Serialize};

use crate::agent::{AgentOptions, AgentRun, RunKind, STEPS};
use crate::attention::Rule;
use crate::demo;
use crate::model::{Checks, Item, Person, Priority, TicketStatus};
use crate::notify::{Channel, NotifyReason, QuietHours};
use crate::review::{Decision, MergeBlocker, MergeMethod, SendError, Verdict};
use crate::rules::{PathViolation, RepoRules};
use crate::sync::{self, Remote};
use crate::usage::{AgentLimits, StopReason, format_euro};
use crate::workspace::{AgentError, LinkError, ResumeError, StatusChange, Workspace};

/// So viele Einträge behält das Protokoll. Ältere fallen heraus.
pub const MAX_LOG: usize = 500;

/// Eine Änderung am Workspace. Eine Variante pro Handgriff der Oberfläche.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    // Inbox
    /// Eintrag als erledigt markieren (`item` ist die interne ID, z. B. `bau142`).
    MarkDone {
        item: String,
    },
    SetStatus {
        item: String,
        status: TicketStatus,
    },
    CreateBranch {
        item: String,
    },
    Reply {
        item: String,
        text: String,
    },
    /// Eintrag zurückstellen. `until_ms` ist der Zeitpunkt (Millisekunden seit 1970), zu
    /// dem er zurückkommt; ohne (`None`) bleibt er weg, bis sich etwas ändert: ein neuer
    /// Kommentar, ein anderer Status oder neue Check-Ergebnisse.
    Snooze {
        item: String,
        until_ms: Option<u64>,
    },
    /// Zurückgestellten Eintrag sofort zurückholen.
    Unsnooze {
        item: String,
    },
    /// Der Takt: Einträge zurückholen, deren Zurückstellung abgelaufen oder durch eine
    /// Änderung aufgehoben ist.
    WakeSnoozed,
    /// Aufmerksamkeits-Regel ein- oder ausschalten.
    SetRule {
        rule: Rule,
        enabled: bool,
    },
    // Tickets bearbeiten
    /// Titel und Beschreibung eines Tickets ändern. Der Titel darf nicht leer sein.
    EditTicket {
        item: String,
        title: String,
        description: String,
    },
    /// Ticket jemandem zuweisen (`person` muss im Team vorkommen, siehe [`Workspace::people`]).
    AssignTicket {
        item: String,
        person: Person,
    },
    SetPriority {
        item: String,
        priority: Priority,
    },
    // Ticket und Pull Request
    /// Ticket (`item`) mit einem Pull Request (`pr`) verknüpfen.
    LinkPr {
        item: String,
        pr: u32,
    },
    /// Verknüpfung eines Tickets lösen. Die Automatik setzt sie nicht wieder.
    UnlinkPr {
        item: String,
    },
    /// Der Takt: Pull Requests verknüpfen, deren Branch oder Titel einen Ticket-Schlüssel nennt.
    AutoLinkPrs,
    /// Für den Pull Request ein Review anfragen.
    RequestReview {
        review: u32,
    },
    /// Der Takt: den Status verknüpfter Tickets dem Pull Request nachführen.
    SyncTicketStatus,
    /// „Status folgt dem Pull Request“ ein- oder ausschalten.
    SetStatusFollowsPr {
        enabled: bool,
    },
    /// Jira-Projekt wählen (Schlüssel wie `BAU`), `None` = alle Projekte. Im Server-Modus holt
    /// der nächste Abgleich dann alle offenen Tickets dieses Projekts.
    SelectJiraProject {
        project: Option<String>,
    },
    // Review
    Decide {
        review: u32,
        suggestion: String,
        decision: Option<Decision>,
    },
    EditSuggestion {
        review: u32,
        suggestion: String,
        text: String,
    },
    /// Eigener Kommentar an einer Diff-Zeile, optional mit Ersatz-Code für die Zeile.
    AddLineComment {
        review: u32,
        file: String,
        line: usize,
        text: String,
        #[serde(default)]
        suggestion: Option<String>,
    },
    /// Ersetzt Text und Vorschlag; ohne `suggestion` entfällt der Vorschlag.
    EditLineComment {
        review: u32,
        comment: String,
        text: String,
        #[serde(default)]
        suggestion: Option<String>,
    },
    DeleteLineComment {
        review: u32,
        comment: String,
    },
    SetVerdict {
        review: u32,
        verdict: Verdict,
    },
    RequestAgentReview {
        review: u32,
    },
    SendReview {
        review: u32,
        comment: String,
    },
    /// Startet einen fehlgeschlagenen Check neu (`check` ist sein Name).
    RestartCheck {
        review: u32,
        check: String,
    },
    /// Der Takt der Checks: jeder laufende kommt einen Schritt weiter.
    AdvanceChecks,
    /// Mergt den Pull Request. Das darf nur `Actor::Du`.
    MergePr {
        review: u32,
        method: MergeMethod,
    },
    // Agent (`ticket` ist der Schlüssel, z. B. `BAU-142`)
    StartAgent {
        ticket: String,
        options: AgentOptions,
    },
    CancelAgent {
        ticket: String,
    },
    ApprovePlan {
        ticket: String,
    },
    RejectPlan {
        ticket: String,
    },
    RevisePlan {
        ticket: String,
        note: String,
    },
    /// Antwort auf eine Rückfrage des Agenten (`run` ist der Ticket-Schlüssel).
    AnswerAgent {
        run: String,
        text: String,
    },
    /// Hinweis an den laufenden Agenten, den er beim nächsten Schritt berücksichtigt.
    SteerAgent {
        run: String,
        text: String,
    },
    PauseAgent {
        run: String,
    },
    ResumeAgent {
        run: String,
    },
    /// Der Takt: jeder laufende Agent schließt einen Schritt ab.
    AdvanceAgents,
    /// Folgeauftrag: Der Agent setzt Kommentare eines Reviews um (IDs der Zeilenkommentare)
    /// und pusht auf den Branch des Pull Requests. Der Lauf heißt dann `review-<Nummer>`.
    AddressReview {
        review: u32,
        comments: Vec<String>,
        options: AgentOptions,
    },
    /// Folgeauftrag: Der Agent untersucht einen roten Check und legt eine Diagnose vor.
    /// Der Lauf heißt dann `check-<Nummer>-<Check>`.
    InvestigateCheck {
        review: u32,
        check: String,
    },
    /// Übernimmt den Fix der Diagnose (`run` ist der Schlüssel des Laufs): Der Agent pusht ihn
    /// auf den Branch und startet den Check neu.
    ApplyFix {
        run: String,
    },
    /// Verwirft die Diagnose. Der Lauf endet, der Branch bleibt unberührt.
    DiscardFix {
        run: String,
    },
    /// Setzt einen angehaltenen Lauf fort, nachdem die Grenze, die ihn angehalten hat, um `extra`
    /// erhöht wurde: Cent bei Budgets, Minuten beim Zeitlimit. Mit `0` geht es weiter, wenn die
    /// Grenze inzwischen gar nicht mehr erreicht ist (z. B. nach Änderung der Einstellung).
    ResumeStopped {
        run: String,
        extra: u64,
    },
    /// Preise und Grenzen der Agenten festlegen.
    SetAgentLimits {
        limits: AgentLimits,
    },
    /// Regeln eines Repos festlegen (`repo` ist sein Name, z. B. `owner/name`).
    SetRepoRules {
        repo: String,
        rules: RepoRules,
    },
    // Benachrichtigungen
    /// Kanal für einen Grund an- oder ausschalten.
    SetNotifyChannel {
        reason: NotifyReason,
        channel: Channel,
        on: bool,
    },
    /// Ruhezeiten setzen (Minuten seit Mitternacht, siehe [`QuietHours`]).
    SetQuietHours {
        quiet: QuietHours,
    },
    // Sonstiges
    /// Zurück zu den Beispieldaten. Das Protokoll beginnt neu.
    ResetDemo,
    /// Der Abgleich: führt den Stand von Jira und GitHub mit dem Workspace zusammen
    /// (siehe [`crate::sync`]). Nur das System löst sie aus – das Backend nach jedem Abruf.
    SyncRemote {
        remote: Remote,
    },
}

impl Action {
    /// Aktionen, die nur das Backend selbst auslösen darf (nie die Oberfläche).
    pub fn system_only(&self) -> bool {
        matches!(self, Action::SyncRemote { .. })
    }
}

/// Wer die Aktion ausgelöst hat.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    Du,
    Agent,
    System,
}

impl Actor {
    pub const ALL: [Actor; 3] = [Actor::Du, Actor::Agent, Actor::System];

    pub fn label(self) -> &'static str {
        match self {
            Actor::Du => "Du",
            Actor::Agent => "Agent",
            Actor::System => "System",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Actor::Du => "du",
            Actor::Agent => "agent",
            Actor::System => "system",
        }
    }

    pub fn from_id(id: &str) -> Option<Actor> {
        Actor::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// Worauf sich ein Protokolleintrag bezieht.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Ein Eintrag der Inbox (interne ID).
    Item(String),
    /// Ein Review (Nummer des Pull Requests).
    Review(u32),
    /// Der Lauf eines Agenten (Ticket-Schlüssel).
    Run(String),
    /// Die Einstellungen (Benachrichtigungen).
    Settings,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Laufende Nummer, steigt mit jedem Eintrag.
    pub id: u64,
    /// Zeitpunkt in Millisekunden seit 1970.
    pub at_ms: u64,
    pub actor: Actor,
    #[serde(default)]
    pub target: Option<Target>,
    /// Kurzer deutscher Satz.
    pub text: String,
}

/// Ergebnis einer erfolgreichen Aktion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// Der Satz, der im Protokoll steht (bei mehreren Einträgen: der erste).
    pub summary: String,
    /// Zahl der neuen Protokolleinträge.
    pub logged: usize,
}

/// Warum eine Aktion nicht angewendet wurde. Der Workspace bleibt dann unverändert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    UnknownItem(String),
    UnknownReview(u32),
    UnknownSuggestion(String),
    UnknownComment(String),
    /// Kein Ticket mit diesem Schlüssel.
    UnknownTicket(String),
    /// Text darf nicht leer sein, oder Datei und Zeile gibt es nicht.
    InvalidInput,
    /// Pull Requests haben keinen Status.
    NotATicket,
    BranchExists,
    /// Es ändert sich nichts (schon erledigt, gleicher Status …).
    Unchanged,
    /// Es läuft nichts, was das zuließe (kein wartender Plan, kein aktiver Lauf, kein Agent).
    NothingToDo,
    Send(SendError),
    Agent(AgentError),
    /// Kein Check mit diesem Namen.
    UnknownCheck(String),
    /// Der Pull Request ist gemergt und lässt sich nicht mehr ändern.
    Merged(u32),
    /// Verknüpfen von Ticket und Pull Request geht nicht.
    Link(LinkError),
    /// Diese Person gibt es im Team nicht.
    UnknownPerson(String),
    /// Gemergt wird nur von Hand: weder der Agent noch das System dürfen es.
    MergeNotAllowed(Actor),
    /// So viele Läufe arbeiten schon, wie die Grenze erlaubt (die Zahl).
    ParallelLimit(u32),
    /// Der Plan berührt Dateien, die die Regeln des Repos nicht zulassen.
    PlanViolatesRules(Vec<PathViolation>),
    /// Auch mit der Erhöhung ist die Grenze noch erreicht.
    LimitStillReached(StopReason),
    /// Dieses Repo gibt es nicht.
    UnknownRepo(String),
    /// Diese Aktion löst nur das System aus (der Abgleich).
    SystemOnly,
    /// Es gibt noch Gründe, die gegen das Mergen sprechen.
    MergeBlocked(Vec<MergeBlocker>),
}

impl ActionError {
    /// Erklärung für die Oberfläche.
    pub fn message(&self) -> String {
        match self {
            ActionError::UnknownItem(id) => format!("Eintrag „{id}“ gibt es nicht"),
            ActionError::UnknownReview(n) => format!("Review #{n} gibt es nicht"),
            ActionError::UnknownSuggestion(id) => format!("Vorschlag „{id}“ gibt es nicht"),
            ActionError::UnknownComment(id) => format!("Kommentar „{id}“ gibt es nicht"),
            ActionError::UnknownTicket(key) => format!("Ticket „{key}“ gibt es nicht"),
            ActionError::InvalidInput => "Die Eingabe passt nicht".into(),
            ActionError::NotATicket => "Pull Requests haben keinen Status".into(),
            ActionError::BranchExists => "Es gibt schon einen Branch".into(),
            ActionError::Unchanged => "Es ändert sich nichts".into(),
            ActionError::NothingToDo => "Dafür gibt es gerade nichts zu tun".into(),
            ActionError::Send(SendError::Undecided(n)) => {
                format!("{n} Vorschläge sind noch nicht entschieden")
            }
            ActionError::Send(SendError::AlreadySent) => "Das Review ist schon gesendet".into(),
            ActionError::Send(SendError::UnknownReview) => "Das Review gibt es nicht".into(),
            ActionError::Agent(AgentError::UnknownTicket) => "Dieses Ticket gibt es nicht".into(),
            ActionError::Agent(AgentError::AlreadyRunning) => {
                "Der Agent arbeitet hier schon".into()
            }
            ActionError::Agent(AgentError::UnknownReview) => "Das Review gibt es nicht".into(),
            ActionError::Agent(AgentError::Merged) => {
                "Der Pull Request ist schon gemergt und bleibt, wie er ist".into()
            }
            ActionError::Agent(AgentError::NoComments) => {
                "Wähle mindestens einen offenen Kommentar aus".into()
            }
            ActionError::Agent(AgentError::CheckNotFailed) => {
                "Untersuchen lässt sich nur ein roter Check".into()
            }
            ActionError::Agent(AgentError::ParallelLimit(n)) | ActionError::ParallelLimit(n) => {
                format!(
                    "Es arbeiten schon {n} Agenten – mehr erlaubt die Grenze nicht. Warte, bis einer fertig ist, oder erhöhe die Grenze unter „Grenzen & Regeln“."
                )
            }
            ActionError::PlanViolatesRules(violations) => {
                let reasons: Vec<String> = violations.iter().map(PathViolation::reason).collect();
                format!(
                    "Der Plan lässt sich nicht freigeben: {}. Ändere die Regeln des Repos oder lehne den Plan ab.",
                    reasons.join("; ")
                )
            }
            ActionError::LimitStillReached(reason) => format!(
                "{} – auch mit dieser Erhöhung. Erhöhe um mehr.",
                reason.label()
            ),
            ActionError::UnknownRepo(repo) => format!("Repo „{repo}“ gibt es nicht"),
            ActionError::UnknownCheck(name) => format!("Check „{name}“ gibt es nicht"),
            ActionError::UnknownPerson(name) => format!("{name} gibt es im Team nicht"),
            ActionError::Link(LinkError::UnknownPr(n)) => {
                format!("Pull Request #{n} gibt es nicht")
            }
            ActionError::Link(LinkError::Same) => "Beide sind schon verknüpft".into(),
            ActionError::Link(LinkError::TicketHasPr(n)) => {
                format!("Das Ticket hängt schon an #{n} – löse das zuerst")
            }
            ActionError::Link(LinkError::PrHasTicket(key)) => {
                format!("Der Pull Request gehört schon zu {key}")
            }
            ActionError::Merged(n) => format!("#{n} ist schon gemergt und bleibt, wie es ist"),
            ActionError::MergeNotAllowed(actor) => format!(
                "Nur du darfst mergen – {} nicht",
                match actor {
                    Actor::Agent => "der Agent",
                    Actor::System => "das System",
                    Actor::Du => "du",
                }
            ),
            ActionError::SystemOnly => {
                "Das macht nur das System – der Abgleich läuft im Server".into()
            }
            ActionError::MergeBlocked(blockers) => {
                let reasons: Vec<&str> = blockers.iter().map(|b| b.reason()).collect();
                format!("Mergen geht noch nicht: {}", reasons.join(" "))
            }
        }
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ActionError {}

/// Ein Eintrag, den eine Aktion ins Protokoll schreiben will.
struct Note {
    actor: Option<Actor>,
    target: Option<Target>,
    text: String,
}

impl Note {
    fn new(target: Target, text: impl Into<String>) -> Self {
        Self {
            actor: None,
            target: Some(target),
            text: text.into(),
        }
    }

    /// Ein Eintrag ohne bestimmtes Ziel (Einstellungen).
    fn general(text: impl Into<String>) -> Self {
        Self {
            actor: None,
            target: None,
            text: text.into(),
        }
    }

    fn by(mut self, actor: Actor) -> Self {
        self.actor = Some(actor);
        self
    }
}

/// Fehler beim Beauftragen: Die Grenze paralleler Läufe hat eine eigene Meldung.
fn agent_error(e: AgentError) -> ActionError {
    match e {
        AgentError::ParallelLimit(n) => ActionError::ParallelLimit(n),
        other => ActionError::Agent(other),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn status_label(status: TicketStatus) -> &'static str {
    TicketStatus::OPTIONS
        .iter()
        .find(|(id, _)| *id == status.id())
        .map_or("", |(_, label)| label)
}

/// Protokolleinträge des Systems für Statuswechsel, die der Pull Request ausgelöst hat.
fn status_notes(changes: Vec<StatusChange>) -> Vec<Note> {
    changes
        .into_iter()
        .map(|c| {
            let why = match c.to {
                TicketStatus::Done => format!("#{} ist gemergt", c.pr),
                TicketStatus::Review => format!("für #{} ist ein Review angefragt", c.pr),
                _ => format!("#{} ist noch in Arbeit", c.pr),
            };
            Note::new(
                Target::Item(c.item),
                format!(
                    "Status von {} automatisch auf „{}“ gesetzt – {why}",
                    c.key,
                    status_label(c.to)
                ),
            )
            .by(Actor::System)
        })
        .collect()
}

impl Workspace {
    /// Wendet eine Aktion an und schreibt sie ins Protokoll.
    ///
    /// `actor` ist, wer sie ausgelöst hat, `at_ms` der Zeitpunkt (Millisekunden seit 1970;
    /// die Zeit kommt von außen, damit die Logik testbar bleibt). Bei einem Fehler bleiben
    /// Workspace und Protokoll unverändert.
    pub fn apply(
        &mut self,
        action: Action,
        actor: Actor,
        at_ms: u64,
    ) -> Result<Applied, ActionError> {
        // Mergen ist Sache des Nutzers: Das prüft schon die Schicht, nicht erst die Oberfläche.
        if matches!(action, Action::MergePr { .. }) && actor != Actor::Du {
            return Err(ActionError::MergeNotAllowed(actor));
        }
        if action.system_only() && actor != Actor::System {
            return Err(ActionError::SystemOnly);
        }
        let notes = self.perform(action, at_ms)?;
        let mut summary = String::new();
        let logged = notes.len();
        for note in notes {
            if summary.is_empty() {
                summary = note.text.clone();
            }
            self.push_log(at_ms, note.actor.unwrap_or(actor), note.target, note.text);
        }
        Ok(Applied { summary, logged })
    }

    fn push_log(&mut self, at_ms: u64, actor: Actor, target: Option<Target>, text: String) {
        let id = self.log.last().map_or(1, |e| e.id + 1);
        self.log.push(LogEntry {
            id,
            at_ms,
            actor,
            target,
            text,
        });
        if self.log.len() > MAX_LOG {
            let excess = self.log.len() - MAX_LOG;
            self.log.drain(..excess);
        }
    }

    /// Das Protokoll, neueste Einträge zuerst. Mit `actor` nur dessen Einträge.
    pub fn log_entries(&self, actor: Option<Actor>) -> Vec<&LogEntry> {
        self.log
            .iter()
            .rev()
            .filter(|e| actor.is_none_or(|a| e.actor == a))
            .collect()
    }

    fn key_of(&self, id: &str) -> Result<String, ActionError> {
        self.item(id)
            .map(|i| i.key.clone())
            .ok_or_else(|| ActionError::UnknownItem(id.to_string()))
    }

    fn need_review(&self, number: u32) -> Result<(), ActionError> {
        self.review(number)
            .map(|_| ())
            .ok_or(ActionError::UnknownReview(number))
    }

    /// Wie `need_review`, aber ein gemergter Pull Request lässt sich nicht mehr ändern.
    fn need_open_review(&self, number: u32) -> Result<(), ActionError> {
        match self.review(number) {
            None => Err(ActionError::UnknownReview(number)),
            Some(r) if r.merged.is_some() => Err(ActionError::Merged(number)),
            Some(_) => Ok(()),
        }
    }

    /// Führt die Aktion aus und liefert die Protokolleinträge dazu.
    fn perform(&mut self, action: Action, at_ms: u64) -> Result<Vec<Note>, ActionError> {
        match action {
            Action::MarkDone { item } => {
                let key = self.key_of(&item)?;
                if self.item(&item).is_some_and(|i| i.read) {
                    return Err(ActionError::Unchanged);
                }
                self.mark_done(&item);
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("{key} als erledigt markiert"),
                )])
            }
            Action::SetStatus { item, status } => {
                let key = self.key_of(&item)?;
                let current = self.item(&item).map(|i| (i.is_ticket(), i.status));
                match current {
                    Some((false, _)) => return Err(ActionError::NotATicket),
                    Some((_, now)) if now == status => return Err(ActionError::Unchanged),
                    _ => {}
                }
                self.set_status(&item, status);
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Status von {key} auf „{}“ gesetzt", status_label(status)),
                )])
            }
            Action::CreateBranch { item } => {
                let key = self.key_of(&item)?;
                let name = self.create_branch(&item).ok_or(ActionError::BranchExists)?;
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Branch {name} für {key} erstellt"),
                )])
            }
            Action::Reply { item, text } => {
                let key = self.key_of(&item)?;
                if !self.reply(&item, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Antwort an {key} gesendet"),
                )])
            }
            Action::Snooze { item, until_ms } => {
                let key = self.key_of(&item)?;
                if until_ms.is_some_and(|t| t <= at_ms) {
                    return Err(ActionError::InvalidInput);
                }
                self.snooze(&item, until_ms);
                let text = match until_ms {
                    Some(_) => format!("{key} zurückgestellt"),
                    None => format!("{key} zurückgestellt, bis sich etwas ändert"),
                };
                Ok(vec![Note::new(Target::Item(item), text)])
            }
            Action::Unsnooze { item } => {
                let key = self.key_of(&item)?;
                if !self.unsnooze(&item) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("{key} aus der Zurückstellung geholt"),
                )])
            }
            Action::WakeSnoozed => {
                let back = self.wake(at_ms);
                if back.is_empty() {
                    return Err(ActionError::NothingToDo);
                }
                Ok(back
                    .into_iter()
                    .map(|id| {
                        let key = self.item(&id).map_or(id.clone(), |i| i.key.clone());
                        Note::new(Target::Item(id), format!("{key} ist zurück in der Inbox"))
                            .by(Actor::System)
                    })
                    .collect())
            }
            Action::SetRule { rule, enabled } => {
                if !self.set_rule(rule, enabled) {
                    return Err(ActionError::Unchanged);
                }
                Ok(vec![Note::general(format!(
                    "Regel „{}“ {}",
                    rule.label(),
                    if enabled {
                        "eingeschaltet"
                    } else {
                        "ausgeschaltet"
                    }
                ))])
            }
            Action::EditTicket {
                item,
                title,
                description,
            } => {
                let key = self.key_of(&item)?;
                let current = self.item(&item).expect("key_of kennt den Eintrag");
                if !current.is_ticket() {
                    return Err(ActionError::NotATicket);
                }
                if title.trim().is_empty() {
                    return Err(ActionError::InvalidInput);
                }
                let title_changed = current.title != title.trim();
                let description_changed = current.description != description.trim();
                if !title_changed && !description_changed {
                    return Err(ActionError::Unchanged);
                }
                self.edit_ticket(&item, &title, &description);
                let what = match (title_changed, description_changed) {
                    (true, true) => "Titel und Beschreibung",
                    (true, false) => "Titel",
                    _ => "Beschreibung",
                };
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("{what} von {key} geändert"),
                )])
            }
            Action::AssignTicket { item, person } => {
                let key = self.key_of(&item)?;
                let current = self.item(&item).expect("key_of kennt den Eintrag");
                if !current.is_ticket() {
                    return Err(ActionError::NotATicket);
                }
                if !self.people().contains(&person) {
                    return Err(ActionError::UnknownPerson(person.name));
                }
                if current.owner == person {
                    return Err(ActionError::Unchanged);
                }
                self.assign(&item, &person);
                let text = if person == Person::me() {
                    format!("{key} dir zugewiesen")
                } else {
                    format!("{key} an {} zugewiesen", person.name)
                };
                Ok(vec![Note::new(Target::Item(item), text)])
            }
            Action::SetPriority { item, priority } => {
                let key = self.key_of(&item)?;
                match self.item(&item) {
                    Some(i) if !i.is_ticket() => return Err(ActionError::NotATicket),
                    Some(i) if i.priority == priority => return Err(ActionError::Unchanged),
                    _ => {}
                }
                self.set_priority(&item, priority);
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Priorität von {key} auf „{}“ gesetzt", priority.label()),
                )])
            }
            Action::LinkPr { item, pr } => {
                let key = self.key_of(&item)?;
                if !self.item(&item).is_some_and(Item::is_ticket) {
                    return Err(ActionError::NotATicket);
                }
                self.link_pr(&item, pr).map_err(|e| match e {
                    LinkError::Same => ActionError::Unchanged,
                    e => ActionError::Link(e),
                })?;
                let mut notes = vec![Note::new(
                    Target::Item(item),
                    format!("{key} mit #{pr} verknüpft"),
                )];
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::UnlinkPr { item } => {
                let key = self.key_of(&item)?;
                if !self.item(&item).is_some_and(Item::is_ticket) {
                    return Err(ActionError::NotATicket);
                }
                let number = self.unlink_pr(&item).ok_or(ActionError::NothingToDo)?;
                Ok(vec![Note::new(
                    Target::Item(item),
                    format!("Verknüpfung von {key} mit #{number} gelöst"),
                )])
            }
            Action::AutoLinkPrs => {
                let linked = self.auto_link();
                if linked.is_empty() {
                    return Err(ActionError::NothingToDo);
                }
                let mut notes: Vec<Note> = linked
                    .into_iter()
                    .map(|(id, key, number)| {
                        Note::new(
                            Target::Item(id),
                            format!("{key} automatisch mit #{number} verknüpft"),
                        )
                        .by(Actor::System)
                    })
                    .collect();
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::RequestReview { review } => {
                self.need_open_review(review)?;
                if !self.request_review(review) {
                    return Err(ActionError::Unchanged);
                }
                let mut notes = vec![Note::new(
                    Target::Review(review),
                    format!("Review für #{review} angefragt"),
                )];
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::SyncTicketStatus => {
                // Ein Fehler ändert nichts: Erst prüfen, ob sich ein Pull Request bewegt hat.
                if !self.status_pending() {
                    return Err(ActionError::NothingToDo);
                }
                // Passt der Status schon, wird der Stand nur vermerkt – ohne Eintrag.
                Ok(status_notes(self.follow_status()))
            }
            Action::SetStatusFollowsPr { enabled } => {
                if !self.set_status_follows_pr(enabled) {
                    return Err(ActionError::Unchanged);
                }
                let mut notes = vec![Note::new(
                    Target::Settings,
                    format!(
                        "Status folgt dem Pull Request: {}",
                        if enabled {
                            "eingeschaltet"
                        } else {
                            "ausgeschaltet"
                        }
                    ),
                )];
                // Wieder eingeschaltet: Was sich inzwischen bewegt hat, wird nachgeführt.
                notes.extend(status_notes(self.follow_status()));
                Ok(notes)
            }
            Action::SelectJiraProject { project } => {
                let project = project
                    .map(|p| p.trim().to_uppercase())
                    .filter(|p| !p.is_empty());
                if project
                    .as_deref()
                    .is_some_and(|p| !crate::is_project_key(p))
                {
                    return Err(ActionError::InvalidInput);
                }
                let text = match &project {
                    Some(p) => format!("Jira-Projekt gewählt: {p}"),
                    None => "Jira-Projekt: alle".to_string(),
                };
                if !self.set_jira_project(project) {
                    return Err(ActionError::Unchanged);
                }
                Ok(vec![Note::new(Target::Settings, text)])
            }
            Action::Decide {
                review,
                suggestion,
                decision,
            } => {
                self.need_open_review(review)?;
                if !self.decide(review, &suggestion, decision) {
                    return Err(ActionError::UnknownSuggestion(suggestion));
                }
                let text = match decision {
                    Some(Decision::Approved) => {
                        format!("Vorschlag {suggestion} in #{review} freigegeben")
                    }
                    Some(Decision::Declined) => {
                        format!("Vorschlag {suggestion} in #{review} abgelehnt")
                    }
                    None => format!(
                        "Entscheidung zu Vorschlag {suggestion} in #{review} zurückgenommen"
                    ),
                };
                Ok(vec![Note::new(Target::Review(review), text)])
            }
            Action::EditSuggestion {
                review,
                suggestion,
                text,
            } => {
                self.need_open_review(review)?;
                let known = self
                    .review(review)
                    .is_some_and(|r| r.suggestions.iter().any(|s| s.id == suggestion));
                if !known {
                    return Err(ActionError::UnknownSuggestion(suggestion));
                }
                if !self.edit_suggestion(review, &suggestion, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Vorschlag {suggestion} in #{review} bearbeitet"),
                )])
            }
            Action::AddLineComment {
                review,
                file,
                line,
                text,
                suggestion,
            } => {
                self.need_open_review(review)?;
                if self
                    .add_line_comment(review, &file, line, &text, suggestion.as_deref())
                    .is_none()
                {
                    return Err(ActionError::InvalidInput);
                }
                let with = if suggestion.is_some() {
                    " mit Änderungsvorschlag"
                } else {
                    ""
                };
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!(
                        "Kommentar{with} an {file}, Zeile {}, in #{review} hinzugefügt",
                        line + 1
                    ),
                )])
            }
            Action::EditLineComment {
                review,
                comment,
                text,
                suggestion,
            } => {
                self.need_open_review(review)?;
                let known = self
                    .review(review)
                    .is_some_and(|r| r.line_comments.iter().any(|c| c.id == comment));
                if !known {
                    return Err(ActionError::UnknownComment(comment));
                }
                if !self.edit_line_comment(review, &comment, &text, suggestion.as_deref()) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Eigenen Kommentar in #{review} bearbeitet"),
                )])
            }
            Action::DeleteLineComment { review, comment } => {
                self.need_open_review(review)?;
                if !self.delete_line_comment(review, &comment) {
                    return Err(ActionError::UnknownComment(comment));
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Eigenen Kommentar in #{review} gelöscht"),
                )])
            }
            Action::SetVerdict { review, verdict } => {
                self.need_open_review(review)?;
                if self.review(review).is_some_and(|r| r.verdict == verdict) {
                    return Err(ActionError::Unchanged);
                }
                self.set_verdict(review, verdict);
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Ergebnis von #{review} auf „{}“ gesetzt", verdict.label()),
                )])
            }
            Action::RequestAgentReview { review } => {
                self.need_open_review(review)?;
                let count = self.request_agent_review(review);
                let result = match count {
                    0 => "Nichts zu beanstanden".to_string(),
                    n => format!("{} vorbereitet", plural(n, "Kommentar", "Kommentare")),
                };
                Ok(vec![
                    Note::new(
                        Target::Review(review),
                        format!("Agent-Review für #{review} angefordert"),
                    ),
                    Note::new(
                        Target::Review(review),
                        format!("#{review} durchgesehen: {result}"),
                    )
                    .by(Actor::Agent),
                ])
            }
            Action::SendReview { review, comment } => {
                let count = self.send_review(review, &comment).map_err(|e| match e {
                    SendError::UnknownReview => ActionError::UnknownReview(review),
                    e => ActionError::Send(e),
                })?;
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!(
                        "Review für #{review} gesendet ({})",
                        plural(count, "Kommentar", "Kommentare")
                    ),
                )])
            }
            Action::RestartCheck { review, check } => {
                self.need_open_review(review)?;
                let known = self
                    .review(review)
                    .is_some_and(|r| r.check_runs.iter().any(|c| c.name == check));
                if !known {
                    return Err(ActionError::UnknownCheck(check));
                }
                if !self.restart_check(review, &check) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Check „{check}“ in #{review} neu gestartet"),
                )])
            }
            Action::AdvanceChecks => {
                if self.running_checks() == 0 {
                    return Err(ActionError::NothingToDo);
                }
                Ok(self
                    .advance_checks()
                    .into_iter()
                    .map(|(number, check, status)| {
                        let result = if status == Checks::Passed {
                            "bestanden"
                        } else {
                            "wieder fehlgeschlagen"
                        };
                        Note::new(
                            Target::Review(number),
                            format!("Check „{check}“ in #{number} {result}"),
                        )
                        .by(Actor::System)
                    })
                    .collect())
            }
            Action::MergePr { review, method } => {
                self.need_review(review)?;
                let changes = self
                    .merge_pr(review, method)
                    .map_err(ActionError::MergeBlocked)?;
                let mut notes = vec![Note::new(
                    Target::Review(review),
                    format!("#{review} gemergt ({})", method.label()),
                )];
                notes.extend(status_notes(changes));
                Ok(notes)
            }
            Action::StartAgent { ticket, options } => {
                self.start_agent(&ticket, options).map_err(agent_error)?;
                // Die Regeln des Repos können einen Plan erzwingen, auch wenn keiner gewünscht war.
                let plan_first = self
                    .runs
                    .iter()
                    .any(|r| r.key() == ticket && r.is_awaiting());
                let mut notes = vec![Note::new(
                    Target::Run(ticket.clone()),
                    format!("Agent für {ticket} beauftragt"),
                )];
                if plan_first {
                    notes.push(
                        Note::new(
                            Target::Run(ticket.clone()),
                            format!("Plan für {ticket} vorgelegt, wartet auf Freigabe"),
                        )
                        .by(Actor::Agent),
                    );
                }
                Ok(notes)
            }
            Action::CancelAgent { ticket } => {
                let Some(label) = self
                    .runs
                    .iter()
                    .find(|r| r.key() == ticket && r.is_active())
                    .map(AgentRun::label)
                else {
                    return Err(ActionError::NothingToDo);
                };
                self.cancel_agent(&ticket);
                Ok(vec![Note::new(
                    Target::Run(ticket),
                    format!("Lauf des Agenten für {label} abgebrochen"),
                )])
            }
            Action::ApprovePlan { ticket } => {
                let label = self.label_of(&ticket);
                if let Some(run) = self
                    .runs
                    .iter()
                    .find(|r| r.key() == ticket && r.is_awaiting())
                {
                    let violations = run.violations();
                    if !violations.is_empty() {
                        return Err(ActionError::PlanViolatesRules(violations));
                    }
                    if let Some(max) = self.parallel_full() {
                        return Err(ActionError::ParallelLimit(max));
                    }
                }
                if !self.approve_plan(&ticket) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(ticket),
                    format!("Plan für {label} freigegeben"),
                )])
            }
            Action::RejectPlan { ticket } => {
                let label = self.label_of(&ticket);
                if !self.reject_plan(&ticket) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(ticket),
                    format!("Plan für {label} abgelehnt"),
                )])
            }
            Action::RevisePlan { ticket, note } => {
                let waiting = self
                    .runs
                    .iter()
                    .any(|r| r.key() == ticket && r.is_awaiting());
                if !waiting {
                    return Err(ActionError::NothingToDo);
                }
                let label = self.label_of(&ticket);
                if !self.revise_plan(&ticket, &note) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Run(ticket),
                    format!("Neuen Plan für {label} angefordert"),
                )])
            }
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
            Action::AnswerAgent { run, text } => {
                if !self.runs.iter().any(|r| r.key() == run && r.is_asking()) {
                    return Err(ActionError::NothingToDo);
                }
                let label = self.label_of(&run);
                if !self.answer_agent(&run, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!(
                        "Frage des Agenten für {label} beantwortet: „{}“",
                        text.trim()
                    ),
                )])
            }
            Action::SteerAgent { run, text } => {
                if !self
                    .runs
                    .iter()
                    .any(|r| r.key() == run && r.is_active() && !r.is_awaiting())
                {
                    return Err(ActionError::NothingToDo);
                }
                let label = self.label_of(&run);
                if !self.steer_agent(&run, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!("Hinweis an den Agenten für {label}: „{}“", text.trim()),
                )])
            }
            Action::PauseAgent { run } => {
                let label = self.label_of(&run);
                if !self.pause_agent(&run) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!("Agent für {label} pausiert"),
                )])
            }
            Action::ResumeAgent { run } => {
                let label = self.label_of(&run);
                if self.runs.iter().any(|r| r.key() == run && r.is_paused())
                    && let Some(max) = self.parallel_full()
                {
                    return Err(ActionError::ParallelLimit(max));
                }
                if !self.resume_agent(&run) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!("Agent für {label} fortgesetzt"),
                )])
            }
            Action::AdvanceAgents => self.advance_logged(at_ms),
            Action::AddressReview {
                review,
                comments,
                options,
            } => {
                self.need_open_review(review)?;
                // Unbekannte Kommentare melden sich beim Namen, erledigte zählen wie fehlende.
                if let Some(id) = comments.iter().find(|id| {
                    self.review(review)
                        .is_some_and(|r| !r.line_comments.iter().any(|c| &c.id == *id))
                }) {
                    return Err(ActionError::UnknownComment(id.clone()));
                }
                let plan_first = options.plan_first;
                let n = comments.len();
                let key = self
                    .address_review(review, &comments, options)
                    .map_err(ActionError::Agent)?;
                let mut notes = vec![Note::new(
                    Target::Review(review),
                    format!(
                        "Agent beauftragt, {} aus #{review} umzusetzen",
                        plural(n, "Kommentar", "Kommentare")
                    ),
                )];
                if plan_first {
                    notes.push(
                        Note::new(
                            Target::Run(key),
                            format!("Plan für die Kommentare in #{review} vorgelegt, wartet auf Freigabe"),
                        )
                        .by(Actor::Agent),
                    );
                }
                Ok(notes)
            }
            Action::InvestigateCheck { review, check } => {
                self.need_open_review(review)?;
                let known = self
                    .review(review)
                    .is_some_and(|r| r.check_runs.iter().any(|c| c.name == check));
                if !known {
                    return Err(ActionError::UnknownCheck(check));
                }
                self.investigate_check(review, &check)
                    .map_err(ActionError::Agent)?;
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Agent beauftragt, den Check „{check}“ in #{review} zu untersuchen"),
                )])
            }
            Action::ApplyFix { run } => {
                let label = self.label_of(&run);
                let pushes = self
                    .runs
                    .iter()
                    .find(|r| r.key() == run && r.is_diagnosed())
                    .and_then(|r| r.diagnosis.as_ref())
                    .is_none_or(|d| d.changes_code());
                if !self.accept_fix(&run) {
                    return Err(ActionError::NothingToDo);
                }
                let text = if pushes {
                    format!(
                        "Fix für {label} übernommen: Der Agent pusht ihn und startet den Check neu"
                    )
                } else {
                    format!("Vorschlag für {label} übernommen: Der Agent startet den Check neu")
                };
                Ok(vec![Note::new(Target::Run(run), text)])
            }
            Action::DiscardFix { run } => {
                let label = self.label_of(&run);
                if !self.discard_fix(&run) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(run),
                    format!("Diagnose für {label} verworfen, am Branch ändert sich nichts"),
                )])
            }
            Action::ResumeStopped { run, extra } => {
                let reason = self
                    .resume_stopped(&run, extra, at_ms)
                    .map_err(|e| match e {
                        ResumeError::NotStopped => ActionError::NothingToDo,
                        ResumeError::ParallelLimit(n) => ActionError::ParallelLimit(n),
                        ResumeError::StillReached(r) => ActionError::LimitStillReached(r),
                    })?;
                let text = if extra == 0 {
                    format!("Agent für {run} fortgesetzt")
                } else {
                    format!(
                        "Agent für {run} fortgesetzt – {} um {} erhöht",
                        reason.raises(),
                        reason.raise_label(extra)
                    )
                };
                Ok(vec![Note::new(Target::Run(run.clone()), text)])
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
                if !self.repos().contains(&repo) {
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
                // Den Satz im Ergebnis liefert die erste Änderung; ohne Änderung bleibt er leer.
                notes.truncate(MAX_LOG);
                Ok(notes)
            }
            Action::ResetDemo => {
                // Die Einstellungen gehören nicht zu den Beispieldaten und bleiben.
                let notifications = std::mem::take(&mut self.notifications);
                let sources = std::mem::take(&mut self.sync.sources);
                let pr_status_off = self.pr_status_off;
                let limits = self.limits.clone();
                let repo_rules = std::mem::take(&mut self.repo_rules);
                let jira_project = self.jira_project.take();
                *self = demo::workspace();
                self.jira_project = jira_project;
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
        }
    }

    /// Der Name des Laufs für Protokollsätze: der Ticket-Schlüssel, bei Folgeaufträgen mit
    /// Klartext. Ein unbekannter Schlüssel bleibt, wie er ist.
    fn label_of(&self, key: &str) -> String {
        self.runs
            .iter()
            .find(|r| r.key() == key)
            .map_or_else(|| key.to_string(), AgentRun::label)
    }

    /// Der Takt der Agenten, mit einem Eintrag pro abgeschlossenem Schritt oder gestellter Frage.
    fn advance_logged(&mut self, at_ms: u64) -> Result<Vec<Note>, ActionError> {
        // Vorher festhalten, welcher Schritt gleich fertig wird und welche Hinweise warten.
        let stepping: Vec<(String, usize, String, Option<u32>, bool, usize)> = self
            .runs
            .iter()
            .filter(|r| r.is_running())
            .map(|r| {
                let title = r.steps()[r.completed.min(STEPS - 1)].title.clone();
                (
                    r.key(),
                    r.completed,
                    title,
                    r.pr_number,
                    r.options.draft_pr,
                    r.pending_hints.len(),
                )
            })
            .collect();
        if stepping.is_empty() {
            return Err(ActionError::NothingToDo);
        }
        self.advance_agents_at(at_ms);
        let mut notes = Vec::new();
        for (key, done, title, pr, draft, hints) in stepping {
            let Some(run) = self.runs.iter().find(|r| r.key() == key) else {
                continue;
            };
            let label = run.label();
            let (asking, diagnosed) = (run.is_asking(), run.is_diagnosed());
            let kind = run.kind.clone();
            if hints > 0 {
                notes.push(
                    Note::new(
                        Target::Run(key.clone()),
                        format!(
                            "{label}: {} berücksichtigt",
                            plural(hints, "Hinweis", "Hinweise")
                        ),
                    )
                    .by(Actor::Agent),
                );
            }
            // Eine Grenze hat den Lauf angehalten, bevor er den Schritt gemacht hat.
            if let Some(reason) = run.stop_reason() {
                notes.push(Note::new(
                    Target::Run(key.clone()),
                    format!("{label}: Angehalten – {}", reason.label()),
                ));
                continue;
            }
            let last = done + 1 == STEPS;
            notes.push(match (asking, diagnosed, last, pr, kind) {
                (true, ..) => Note::new(
                    Target::Run(key.clone()),
                    format!("{label}: Der Agent hat eine Frage und wartet auf deine Antwort"),
                ),
                (_, true, ..) => Note::new(
                    Target::Run(key.clone()),
                    format!("{label}: Diagnose liegt vor, der Agent wartet auf deine Entscheidung"),
                ),
                (_, _, true, Some(n), RunKind::AddressReview { comments, .. }) => Note::new(
                    Target::Review(n),
                    format!(
                        "#{n}: {} umgesetzt und auf den Branch gepusht, das Review wartet wieder auf dich",
                        plural(comments.len(), "Kommentar", "Kommentare")
                    ),
                ),
                (_, _, true, Some(n), RunKind::Diagnose { check, .. }) => Note::new(
                    Target::Review(n),
                    format!("#{n}: Check „{check}“ nach dem Fix neu gestartet"),
                ),
                (_, _, true, Some(n), RunKind::Implement) => Note::new(
                    Target::Review(n),
                    format!(
                        "{key}: Pull Request #{n} {}erstellt",
                        if draft { "als Entwurf " } else { "" }
                    ),
                ),
                _ => Note::new(Target::Run(key.clone()), format!("{label}: {title}")),
            });
        }
        Ok(notes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Filter, ReasonKind, Scope, Tone};
    use crate::notify::NotifySettings;
    use crate::persist;
    use crate::review::{CHECK_STEPS, Decision};

    const T: u64 = 1_700_000_000_000;

    fn ws() -> Workspace {
        demo::workspace()
    }

    fn direct() -> AgentOptions {
        AgentOptions {
            plan_first: false,
            ..Default::default()
        }
    }

    /// Wendet an und erwartet genau `n` neue Einträge.
    fn ok(w: &mut Workspace, action: Action, n: usize) -> Applied {
        let before = w.log.len();
        let applied = w.apply(action.clone(), Actor::Du, T).unwrap();
        assert_eq!(applied.logged, n, "{action:?}");
        assert_eq!(w.log.len(), before + n, "{action:?}");
        applied
    }

    fn fails(w: &mut Workspace, action: Action, error: ActionError) {
        let before = w.clone();
        assert_eq!(w.apply(action, Actor::Du, T), Err(error));
        assert_eq!(
            *w, before,
            "ein Fehler ändert nichts und protokolliert nichts"
        );
    }

    #[test]
    fn every_action_variant_is_applied_and_logged() {
        let mut w = ws();
        let file = w.review(87).unwrap().files[0].name.clone();
        let id = "bau142".to_string();
        let key = "BAU-142".to_string();

        ok(&mut w, Action::MarkDone { item: id.clone() }, 1);
        ok(
            &mut w,
            Action::SetStatus {
                item: "bau150".into(),
                status: TicketStatus::Done,
            },
            1,
        );
        ok(
            &mut w,
            Action::CreateBranch {
                item: "bau150".into(),
            },
            1,
        );
        ok(
            &mut w,
            Action::Reply {
                item: id.clone(),
                text: "Mache ich.".into(),
            },
            1,
        );
        ok(
            &mut w,
            Action::Decide {
                review: 87,
                suggestion: "a".into(),
                decision: Some(Decision::Approved),
            },
            1,
        );
        ok(
            &mut w,
            Action::EditSuggestion {
                review: 87,
                suggestion: "a".into(),
                text: "Anders".into(),
            },
            1,
        );
        ok(
            &mut w,
            Action::AddLineComment {
                review: 87,
                file,
                line: 0,
                text: "Warum?".into(),
                suggestion: None,
            },
            1,
        );
        let comment = w.review(87).unwrap().line_comments[0].id.clone();
        ok(
            &mut w,
            Action::EditLineComment {
                review: 87,
                comment: comment.clone(),
                text: "Wieso?".into(),
                suggestion: None,
            },
            1,
        );
        ok(
            &mut w,
            Action::DeleteLineComment {
                review: 87,
                comment,
            },
            1,
        );
        ok(
            &mut w,
            Action::SetVerdict {
                review: 87,
                verdict: Verdict::Approve,
            },
            1,
        );
        ok(&mut w, Action::RequestAgentReview { review: 85 }, 2);
        for s in ["b", "c"] {
            w.decide(87, s, Some(Decision::Declined));
        }
        ok(
            &mut w,
            Action::SendReview {
                review: 87,
                comment: String::new(),
            },
            1,
        );
        ok(
            &mut w,
            Action::StartAgent {
                ticket: key.clone(),
                options: AgentOptions::default(),
            },
            2,
        );
        ok(
            &mut w,
            Action::RevisePlan {
                ticket: key.clone(),
                note: "Doku".into(),
            },
            1,
        );
        ok(
            &mut w,
            Action::ApprovePlan {
                ticket: key.clone(),
            },
            1,
        );
        ok(&mut w, Action::AdvanceAgents, 1);
        ok(
            &mut w,
            Action::SteerAgent {
                run: key.clone(),
                text: "Auch den Slider".into(),
            },
            1,
        );
        ok(&mut w, Action::PauseAgent { run: key.clone() }, 1);
        ok(&mut w, Action::ResumeAgent { run: key.clone() }, 1);
        ok(&mut w, Action::AdvanceAgents, 2);
        ok(&mut w, Action::AdvanceAgents, 1);
        ok(
            &mut w,
            Action::AnswerAgent {
                run: key.clone(),
                text: "Entfernen".into(),
            },
            1,
        );
        ok(
            &mut w,
            Action::CancelAgent {
                ticket: key.clone(),
            },
            1,
        );
        ok(
            &mut w,
            Action::StartAgent {
                ticket: key.clone(),
                options: AgentOptions::default(),
            },
            2,
        );
        ok(&mut w, Action::RejectPlan { ticket: key }, 1);
        let reset = w.apply(Action::ResetDemo, Actor::Du, T).unwrap();
        assert_eq!(reset.logged, 1);
        assert_eq!(
            w.log.len(),
            1,
            "nach dem Zurücksetzen beginnt das Protokoll neu"
        );
        assert_eq!(w.log[0].actor, Actor::System);
        assert_eq!(w.log[0].text, "Demo-Daten zurückgesetzt");
    }

    #[test]
    fn entries_carry_time_actor_target_and_a_german_sentence() {
        let mut w = ws();
        w.apply(
            Action::MarkDone {
                item: "bau142".into(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        let e = &w.log[0];
        assert_eq!(
            (e.id, e.at_ms, e.actor, e.target.clone(), e.text.as_str()),
            (
                1,
                T,
                Actor::Du,
                Some(Target::Item("bau142".into())),
                "BAU-142 als erledigt markiert"
            )
        );
        w.apply(
            Action::SetStatus {
                item: "bau150".into(),
                status: TicketStatus::InProgress,
            },
            Actor::Du,
            T + 5,
        )
        .unwrap();
        assert_eq!(w.log[1].id, 2);
        assert_eq!(
            w.log[1].text,
            "Status von BAU-150 auf „In Arbeit“ ggesetzt".replace("gg", "g")
        );
        assert_eq!(w.log[1].at_ms, T + 5);
    }

    #[test]
    fn failures_are_returned_and_leave_no_entry() {
        let mut w = ws();
        fails(
            &mut w,
            Action::MarkDone {
                item: "gibt-es-nicht".into(),
            },
            ActionError::UnknownItem("gibt-es-nicht".into()),
        );
        w.mark_done("bau142");
        fails(
            &mut w,
            Action::MarkDone {
                item: "bau142".into(),
            },
            ActionError::Unchanged,
        );
        fails(
            &mut w,
            Action::SetStatus {
                item: "pr87".into(),
                status: TicketStatus::Done,
            },
            ActionError::NotATicket,
        );
        fails(
            &mut w,
            Action::SetStatus {
                item: "bau150".into(),
                status: ws().item("bau150").unwrap().status,
            },
            ActionError::Unchanged,
        );
        fails(
            &mut w,
            Action::CreateBranch {
                item: "pr87".into(),
            },
            ActionError::BranchExists,
        );
        fails(
            &mut w,
            Action::Reply {
                item: "bau142".into(),
                text: "  ".into(),
            },
            ActionError::InvalidInput,
        );
        fails(
            &mut w,
            Action::Decide {
                review: 1,
                suggestion: "a".into(),
                decision: None,
            },
            ActionError::UnknownReview(1),
        );
        fails(
            &mut w,
            Action::Decide {
                review: 87,
                suggestion: "zzz".into(),
                decision: None,
            },
            ActionError::UnknownSuggestion("zzz".into()),
        );
        fails(
            &mut w,
            Action::AddLineComment {
                review: 87,
                file: "nope.css".into(),
                line: 0,
                text: "x".into(),
                suggestion: None,
            },
            ActionError::InvalidInput,
        );
        fails(
            &mut w,
            Action::DeleteLineComment {
                review: 87,
                comment: "c9".into(),
            },
            ActionError::UnknownComment("c9".into()),
        );
        fails(
            &mut w,
            Action::SendReview {
                review: 87,
                comment: String::new(),
            },
            ActionError::Send(SendError::Undecided(3)),
        );
        fails(
            &mut w,
            Action::SendReview {
                review: 1,
                comment: String::new(),
            },
            ActionError::UnknownReview(1),
        );
        fails(
            &mut w,
            Action::StartAgent {
                ticket: "#87".into(),
                options: direct(),
            },
            ActionError::Agent(AgentError::UnknownTicket),
        );
        fails(
            &mut w,
            Action::ApprovePlan {
                ticket: "BAU-142".into(),
            },
            ActionError::NothingToDo,
        );
        fails(
            &mut w,
            Action::CancelAgent {
                ticket: "BAU-138".into(),
            },
            ActionError::NothingToDo,
        );
        fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
        assert!(w.log.is_empty());
        assert!(!ActionError::NothingToDo.message().is_empty());
    }

    #[test]
    fn starting_twice_fails() {
        let mut w = ws();
        ok(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: direct(),
            },
            1,
        );
        fails(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: direct(),
            },
            ActionError::Agent(AgentError::AlreadyRunning),
        );
    }

    #[test]
    fn agent_steps_from_the_tick_are_logged_as_agent() {
        let mut w = ws();
        w.apply(
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: direct(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        for i in 0..=STEPS {
            if w.run("BAU-142").unwrap().is_asking() {
                w.apply(
                    Action::AnswerAgent {
                        run: "BAU-142".into(),
                        text: "Entfernen".into(),
                    },
                    Actor::Du,
                    T + 1 + i as u64,
                )
                .unwrap();
            }
            w.apply(Action::AdvanceAgents, Actor::Agent, T + 1 + i as u64)
                .unwrap();
        }
        let steps: Vec<_> = w
            .log_entries(Some(Actor::Agent))
            .into_iter()
            .rev()
            .map(|e| e.text.as_str())
            .collect();
        assert_eq!(
            steps,
            [
                "BAU-142: Ticket gelesen",
                "BAU-142: Branch erstellt",
                "BAU-142: Der Agent hat eine Frage und wartet auf deine Antwort",
                "BAU-142: Änderungen umgesetzt",
                "BAU-142: Tests ausgeführt",
                "BAU-142: Pull Request #92 als Entwurf erstellt",
            ]
        );
        let last = w.log.last().unwrap();
        assert_eq!(last.target, Some(Target::Review(92)));
        assert_eq!(w.log[1].target, Some(Target::Run("BAU-142".into())));
        assert_eq!(w.log_entries(Some(Actor::Du)).len(), 2, "Start und Antwort");
        assert_eq!(w.log_entries(None).len(), 8);
        assert_eq!(w.log_entries(Some(Actor::System)).len(), 0);
        assert_eq!(w.log_entries(None)[0].id, 8, "neueste zuerst");
        // Danach läuft nichts mehr.
        assert_eq!(
            w.apply(Action::AdvanceAgents, Actor::Agent, T),
            Err(ActionError::NothingToDo)
        );
    }

    #[test]
    fn the_two_rules_still_hold() {
        let mut w = ws();
        // Freigabe vor dem Senden.
        assert_eq!(
            w.apply(
                Action::SendReview {
                    review: 87,
                    comment: String::new()
                },
                Actor::Du,
                T
            ),
            Err(ActionError::Send(SendError::Undecided(
                w.review(87).unwrap().undecided()
            )))
        );
        // Der Agent merged nie: kein Merge in den Aktionen, der Pull Request wartet als Review.
        w.apply(
            Action::StartAgent {
                ticket: "BAU-150".into(),
                options: direct(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        for _ in 0..STEPS {
            let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T);
        }
        assert!(!w.review(92).unwrap().sent);
    }

    #[test]
    fn the_log_is_capped_and_ids_keep_rising() {
        let mut w = ws();
        for i in 0..(MAX_LOG + 20) {
            let status = if i % 2 == 0 {
                TicketStatus::Done
            } else {
                TicketStatus::Open
            };
            let _ = w.apply(
                Action::SetStatus {
                    item: "bau150".into(),
                    status,
                },
                Actor::Du,
                T + i as u64,
            );
        }
        assert_eq!(w.log.len(), MAX_LOG);
        assert_eq!(w.log.last().unwrap().id, (MAX_LOG + 20) as u64);
        assert_eq!(w.log[0].id, 21);
    }

    #[test]
    fn notification_settings_are_changed_by_actions() {
        let mut w = ws();
        let mention = |on| Action::SetNotifyChannel {
            reason: NotifyReason::Mention,
            channel: Channel::Slack,
            on,
        };
        let applied = ok(&mut w, mention(true), 1);
        assert_eq!(
            applied.summary,
            "Benachrichtigung „Erwähnung“ über Slack eingeschaltet"
        );
        assert!(
            w.notifications
                .enabled(NotifyReason::Mention, Channel::Slack)
        );
        assert_eq!(w.log.last().unwrap().target, Some(Target::Settings));
        fails(&mut w, mention(true), ActionError::Unchanged);
        let applied = ok(&mut w, mention(false), 1);
        assert!(applied.summary.ends_with("ausgeschaltet"));

        let quiet = QuietHours {
            enabled: true,
            from: 22 * 60,
            to: 7 * 60,
        };
        let applied = ok(&mut w, Action::SetQuietHours { quiet }, 1);
        assert_eq!(
            applied.summary,
            "Ruhezeiten von 22:00 bis 07:00 Uhr eingeschaltet"
        );
        fails(
            &mut w,
            Action::SetQuietHours { quiet },
            ActionError::Unchanged,
        );
        fails(
            &mut w,
            Action::SetQuietHours {
                quiet: QuietHours {
                    to: 24 * 60,
                    ..quiet
                },
            },
            ActionError::InvalidInput,
        );
        let off = QuietHours {
            enabled: false,
            ..quiet
        };
        let applied = ok(&mut w, Action::SetQuietHours { quiet: off }, 1);
        assert_eq!(applied.summary, "Ruhezeiten ausgeschaltet");
    }

    #[test]
    fn reset_keeps_notification_settings() {
        let mut w = ws();
        ok(
            &mut w,
            Action::SetNotifyChannel {
                reason: NotifyReason::Assigned,
                channel: Channel::Browser,
                on: false,
            },
            1,
        );
        w.apply(Action::ResetDemo, Actor::Du, T).unwrap();
        assert!(
            !w.notifications
                .enabled(NotifyReason::Assigned, Channel::Browser)
        );
    }

    #[test]
    fn notification_settings_survive_save_and_load() {
        let mut w = ws();
        w.notifications
            .set(NotifyReason::AgentFinished, Channel::Email, true);
        let back = persist::load(&persist::save(&w)).unwrap();
        assert!(
            back.notifications
                .enabled(NotifyReason::AgentFinished, Channel::Email)
        );
        // Ältere Stände ohne das Feld laden weiter, mit den Standardwerten.
        let mut json: serde_json::Value = serde_json::from_str(&persist::save(&w)).unwrap();
        json["workspace"]
            .as_object_mut()
            .unwrap()
            .remove("notifications");
        let old = persist::load(&json.to_string()).unwrap();
        assert_eq!(old.notifications, NotifySettings::default());
    }

    #[test]
    fn actions_roundtrip_as_json() {
        let action = Action::Decide {
            review: 87,
            suggestion: "a".into(),
            decision: Some(Decision::Declined),
        };
        let text = serde_json::to_string(&action).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&text).unwrap(), action);
        let start = Action::StartAgent {
            ticket: "BAU-1".into(),
            options: AgentOptions::default(),
        };
        let text = serde_json::to_string(&start).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&text).unwrap(), start);
        assert_eq!(
            serde_json::from_str::<Action>("\"AdvanceAgents\"").unwrap(),
            Action::AdvanceAgents
        );
    }

    #[test]
    fn the_log_survives_save_and_load() {
        let mut w = ws();
        w.apply(
            Action::MarkDone {
                item: "bau142".into(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        w.apply(Action::RequestAgentReview { review: 85 }, Actor::Du, T + 1)
            .unwrap();
        let back = persist::load(&persist::save(&w)).unwrap();
        assert_eq!(back.log, w.log);
        assert_eq!(back, w);
    }

    #[test]
    fn old_saves_without_a_log_still_load() {
        let mut value: serde_json::Value = serde_json::from_str(&persist::save(&ws())).unwrap();
        value["workspace"].as_object_mut().unwrap().remove("log");
        let back = persist::load(&value.to_string()).unwrap();
        assert!(back.log.is_empty());
        assert_eq!(back.items.len(), 5);
        // Und es geht weiter.
        let mut back = back;
        back.apply(
            Action::MarkDone {
                item: "bau142".into(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        assert_eq!(back.log.len(), 1);
    }

    #[test]
    fn snoozing_is_an_action_and_comes_back_by_itself() {
        let mut w = ws();
        let hour = 3_600_000;
        let applied = ok(
            &mut w,
            Action::Snooze {
                item: "pr87".into(),
                until_ms: Some(T + hour),
            },
            1,
        );
        assert_eq!(applied.summary, "#87 zurückgestellt");
        assert!(w.is_snoozed("pr87", T));
        // Nichts zu wecken, solange die Zeit nicht um ist.
        fails(&mut w, Action::WakeSnoozed, ActionError::NothingToDo);
        // Der Takt holt ihn zurück und protokolliert es als System.
        let woke = w
            .apply(Action::WakeSnoozed, Actor::System, T + hour)
            .unwrap();
        assert_eq!(woke.summary, "#87 ist zurück in der Inbox");
        assert!(w.snoozed.is_empty());
        assert_eq!(w.log.last().unwrap().actor, Actor::System);

        ok(
            &mut w,
            Action::Snooze {
                item: "pr87".into(),
                until_ms: None,
            },
            1,
        );
        assert_eq!(
            w.log.last().unwrap().text,
            "#87 zurückgestellt, bis sich etwas ändert"
        );
        ok(
            &mut w,
            Action::Unsnooze {
                item: "pr87".into(),
            },
            1,
        );
        fails(
            &mut w,
            Action::Unsnooze {
                item: "pr87".into(),
            },
            ActionError::NothingToDo,
        );
        fails(
            &mut w,
            Action::Snooze {
                item: "gibt-es-nicht".into(),
                until_ms: None,
            },
            ActionError::UnknownItem("gibt-es-nicht".into()),
        );
        // Ein Zeitpunkt in der Vergangenheit ist keine Zurückstellung.
        fails(
            &mut w,
            Action::Snooze {
                item: "pr87".into(),
                until_ms: Some(T),
            },
            ActionError::InvalidInput,
        );
    }

    #[test]
    fn a_change_wakes_a_snooze_and_the_tick_logs_it() {
        let mut w = ws();
        ok(
            &mut w,
            Action::Snooze {
                item: "bau142".into(),
                until_ms: None,
            },
            1,
        );
        fails(&mut w, Action::WakeSnoozed, ActionError::NothingToDo);
        ok(
            &mut w,
            Action::Reply {
                item: "bau142".into(),
                text: "Mache ich.".into(),
            },
            1,
        );
        let woke = w.apply(Action::WakeSnoozed, Actor::System, T).unwrap();
        assert_eq!(woke.summary, "BAU-142 ist zurück in der Inbox");
    }

    #[test]
    fn rules_are_switched_by_action() {
        let mut w = ws();
        let off = ok(
            &mut w,
            Action::SetRule {
                rule: Rule::Overdue,
                enabled: false,
            },
            1,
        );
        assert_eq!(off.summary, "Regel „Überfälliges Ticket“ ausgeschaltet");
        assert!(!w.rules.enabled(Rule::Overdue));
        assert_eq!(w.log.last().unwrap().target, None);
        fails(
            &mut w,
            Action::SetRule {
                rule: Rule::Overdue,
                enabled: false,
            },
            ActionError::Unchanged,
        );
        let on = ok(
            &mut w,
            Action::SetRule {
                rule: Rule::Overdue,
                enabled: true,
            },
            1,
        );
        assert_eq!(on.summary, "Regel „Überfälliges Ticket“ eingeschaltet");
    }

    #[test]
    fn rules_and_snoozes_survive_save_and_load() {
        let mut w = ws();
        w.apply(
            Action::SetRule {
                rule: Rule::Assigned,
                enabled: false,
            },
            Actor::Du,
            T,
        )
        .unwrap();
        w.apply(
            Action::Snooze {
                item: "pr87".into(),
                until_ms: Some(T + 1),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        let back = persist::load(&persist::save(&w)).unwrap();
        assert_eq!(back, w);
        assert!(!back.rules.enabled(Rule::Assigned));
        let text = serde_json::to_string(&Action::Snooze {
            item: "x".into(),
            until_ms: None,
        })
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<Action>(&text),
            Ok(Action::Snooze { until_ms: None, .. })
        ));
    }

    #[test]
    fn old_saves_without_rules_snoozes_or_due_dates_still_load() {
        let mut value: serde_json::Value = serde_json::from_str(&persist::save(&ws())).unwrap();
        let workspace = value["workspace"].as_object_mut().unwrap();
        workspace.remove("rules");
        workspace.remove("snoozed");
        for item in workspace["items"].as_array_mut().unwrap() {
            item.as_object_mut().unwrap().remove("due");
            if let Some(pr) = item.get_mut("pr").and_then(|p| p.as_object_mut()) {
                pr.remove("changes_requested");
            }
        }
        let back = persist::load(&value.to_string()).unwrap();
        assert!(Rule::ALL.iter().all(|r| back.rules.enabled(*r)));
        assert!(back.snoozed.is_empty());
        assert_eq!(back.item("bau150").unwrap().due, None);
    }

    #[test]
    fn actor_ids_roundtrip() {
        for a in Actor::ALL {
            assert_eq!(Actor::from_id(a.id()), Some(a));
        }
        assert_eq!(Actor::from_id("x"), None);
    }

    /// Bringt BAU-142 bis zur Rückfrage des Agenten.
    fn asking_ws() -> Workspace {
        let mut w = ws();
        ok(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: direct(),
            },
            1,
        );
        ok(&mut w, Action::AdvanceAgents, 1);
        ok(&mut w, Action::AdvanceAgents, 1);
        ok(&mut w, Action::AdvanceAgents, 1);
        assert!(w.run("BAU-142").unwrap().is_asking());
        w
    }

    #[test]
    fn answering_continues_the_agent_and_is_logged() {
        let mut w = asking_ws();
        assert_eq!(
            w.item("bau142").unwrap().agent_note.as_deref(),
            Some("Der Agent hat eine Frage an dich")
        );
        let applied = ok(
            &mut w,
            Action::AnswerAgent {
                run: "BAU-142".into(),
                text: " Als deprecated markieren ".into(),
            },
            1,
        );
        assert_eq!(
            applied.summary,
            "Frage des Agenten für BAU-142 beantwortet: „Als deprecated markieren“"
        );
        let last = w.log.last().unwrap();
        assert_eq!(
            (last.actor, &last.target),
            (Actor::Du, &Some(Target::Run("BAU-142".into())))
        );
        assert!(w.run("BAU-142").unwrap().is_running());
        assert_eq!(
            w.item("bau142").unwrap().agent_note.as_deref(),
            Some("Der Agent arbeitet an der Umsetzung")
        );
    }

    #[test]
    fn invalid_answers_change_nothing() {
        let mut w = asking_ws();
        fails(
            &mut w,
            Action::AnswerAgent {
                run: "BAU-142".into(),
                text: "  ".into(),
            },
            ActionError::InvalidInput,
        );
        let mut idle = ws();
        for action in [
            Action::AnswerAgent {
                run: "BAU-142".into(),
                text: "Ja".into(),
            },
            Action::SteerAgent {
                run: "BAU-142".into(),
                text: "Ja".into(),
            },
            Action::PauseAgent {
                run: "BAU-142".into(),
            },
            Action::ResumeAgent {
                run: "BAU-142".into(),
            },
        ] {
            fails(&mut idle, action, ActionError::NothingToDo);
        }
        // Wer fragt, läuft nicht: pausieren geht erst nach der Antwort.
        fails(
            &mut w,
            Action::PauseAgent {
                run: "BAU-142".into(),
            },
            ActionError::NothingToDo,
        );
        fails(
            &mut w,
            Action::ResumeAgent {
                run: "BAU-142".into(),
            },
            ActionError::NothingToDo,
        );
    }

    #[test]
    fn steering_is_acknowledged_by_the_agent_in_the_next_step() {
        let mut w = ws();
        ok(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-150".into(),
                options: direct(),
            },
            1,
        );
        fails(
            &mut w,
            Action::SteerAgent {
                run: "BAU-150".into(),
                text: "".into(),
            },
            ActionError::InvalidInput,
        );
        ok(
            &mut w,
            Action::SteerAgent {
                run: "BAU-150".into(),
                text: "Bitte auch die Doku".into(),
            },
            1,
        );
        // Der Takt protokolliert den berücksichtigten Hinweis und den Schritt.
        let tick = w.apply(Action::AdvanceAgents, Actor::Agent, T).unwrap();
        assert_eq!(tick.logged, 2);
        assert_eq!(tick.summary, "BAU-150: 1 Hinweis berücksichtigt");
        let run = w.run("BAU-150").unwrap();
        assert!(
            run.messages
                .iter()
                .any(|m| m.text == "Hinweis erhalten: Bitte auch die Doku")
        );
    }

    // ---------- Mergen, Checks, Änderungsvorschläge ----------

    /// #87 ist bereit zum Mergen: alle Vorschläge entschieden, Review gesendet.
    fn ready_87(w: &mut Workspace) {
        for id in ["a", "b", "c"] {
            ok(
                w,
                Action::Decide {
                    review: 87,
                    suggestion: id.into(),
                    decision: Some(Decision::Declined),
                },
                1,
            );
        }
        ok(
            w,
            Action::SendReview {
                review: 87,
                comment: String::new(),
            },
            1,
        );
    }

    fn merge(review: u32, method: MergeMethod) -> Action {
        Action::MergePr { review, method }
    }

    #[test]
    fn merging_is_refused_until_every_condition_holds() {
        let mut w = ws();
        fails(
            &mut w,
            merge(87, MergeMethod::Squash),
            ActionError::MergeBlocked(vec![MergeBlocker::ReviewNotSent]),
        );
        // Ein Urteil „Änderungen“ blockiert auch nach dem Senden.
        ok(
            &mut w,
            Action::SetVerdict {
                review: 87,
                verdict: Verdict::RequestChanges,
            },
            1,
        );
        ready_87(&mut w);
        fails(
            &mut w,
            merge(87, MergeMethod::Merge),
            ActionError::MergeBlocked(vec![MergeBlocker::ChangesRequested]),
        );
        // Rote Checks (#85) blockieren, die Begründung steht in der Fehlermeldung.
        let err = w
            .apply(merge(85, MergeMethod::Merge), Actor::Du, T)
            .unwrap_err();
        assert!(err.message().contains("Check ist fehlgeschlagen"));
        // Unbekannt.
        fails(
            &mut w,
            merge(1, MergeMethod::Merge),
            ActionError::UnknownReview(1),
        );
    }

    #[test]
    fn a_paused_agent_makes_no_progress_until_resumed() {
        let mut w = ws();
        ok(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-150".into(),
                options: direct(),
            },
            1,
        );
        ok(
            &mut w,
            Action::PauseAgent {
                run: "BAU-150".into(),
            },
            1,
        );
        assert_eq!(
            w.item("bau150").unwrap().agent_note.as_deref(),
            Some("Der Agent ist pausiert")
        );
        fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
        fails(
            &mut w,
            Action::PauseAgent {
                run: "BAU-150".into(),
            },
            ActionError::NothingToDo,
        );
        // Abbrechen geht auch pausiert.
        ok(
            &mut w,
            Action::ResumeAgent {
                run: "BAU-150".into(),
            },
            1,
        );
        ok(&mut w, Action::AdvanceAgents, 1);
        ok(
            &mut w,
            Action::PauseAgent {
                run: "BAU-150".into(),
            },
            1,
        );
        ok(
            &mut w,
            Action::CancelAgent {
                ticket: "BAU-150".into(),
            },
            1,
        );
        assert!(w.run("BAU-150").unwrap().is_idle());
    }

    #[test]
    fn the_agent_still_never_merges_after_questions() {
        let mut w = asking_ws();
        ok(
            &mut w,
            Action::AnswerAgent {
                run: "BAU-142".into(),
                text: "Entfernen".into(),
            },
            1,
        );
        for _ in 0..STEPS {
            let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T);
        }
        // Am Ende wartet ein Pull Request auf das Review, gemergt ist nichts.
        let review = w.review(92).unwrap();
        assert!(!review.sent);
        assert!(
            w.log
                .iter()
                .all(|e| !e.text.to_lowercase().contains("gemergt"))
        );
    }

    #[test]
    fn new_actions_survive_json() {
        for action in [
            Action::AnswerAgent {
                run: "BAU-142".into(),
                text: "Ja".into(),
            },
            Action::SteerAgent {
                run: "BAU-142".into(),
                text: "Ja".into(),
            },
            Action::PauseAgent {
                run: "BAU-142".into(),
            },
            Action::ResumeAgent {
                run: "BAU-142".into(),
            },
        ] {
            let json = serde_json::to_string(&action).unwrap();
            assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), action);
        }
    }

    #[test]
    fn only_you_may_merge() {
        let mut w = ws();
        ready_87(&mut w);
        let before = w.clone();
        for actor in [Actor::Agent, Actor::System] {
            let err = w
                .apply(merge(87, MergeMethod::Merge), actor, T)
                .unwrap_err();
            assert_eq!(err, ActionError::MergeNotAllowed(actor));
            assert!(err.message().starts_with("Nur du darfst mergen"));
        }
        assert_eq!(w, before, "kein Merge, kein Protokolleintrag");
        w.apply(merge(87, MergeMethod::Merge), Actor::Du, T)
            .unwrap();
        assert!(w.review(87).unwrap().merged.is_some());
    }

    #[test]
    fn merging_marks_the_pull_request_and_finishes_the_ticket() {
        let mut w = ws();
        // #91 gehört zu BAU-138; der Entwurf wird vorher freigegeben.
        w.items
            .iter_mut()
            .find(|i| i.id == "bau138")
            .unwrap()
            .pr
            .as_mut()
            .unwrap()
            .draft = true;
        fails(
            &mut w,
            merge(91, MergeMethod::Rebase),
            ActionError::MergeBlocked(vec![MergeBlocker::Draft, MergeBlocker::ReviewNotSent]),
        );
        w.items
            .iter_mut()
            .find(|i| i.id == "bau138")
            .unwrap()
            .pr
            .as_mut()
            .unwrap()
            .draft = false;
        ok(
            &mut w,
            Action::SendReview {
                review: 91,
                comment: String::new(),
            },
            1,
        );
        let applied = ok(&mut w, merge(91, MergeMethod::Rebase), 2);
        assert_eq!(applied.summary, "#91 gemergt (Rebase)");
        // Das Ticket zieht die Automatik nach, mit dem System als Urheber.
        let entry = w.log.last().unwrap();
        assert_eq!(entry.actor, Actor::System);
        assert_eq!(entry.target, Some(Target::Item("bau138".into())));
        assert_eq!(
            entry.text,
            "Status von BAU-138 automatisch auf „Fertig“ gesetzt – #91 ist gemergt"
        );
        assert_eq!(w.review(91).unwrap().merged, Some(MergeMethod::Rebase));
        let ticket = w.item("bau138").unwrap();
        assert_eq!(ticket.status, TicketStatus::Done);
        assert!(ticket.read);
        let pr = ticket.pr.as_ref().unwrap();
        assert!(pr.merged);
        assert_eq!(crate::team::state_label(ticket).0, "Fertig");
        assert_eq!(pr.status().0, "Gemergt");
        // Zweimal mergen geht nicht, ändern auch nicht.
        fails(
            &mut w,
            merge(91, MergeMethod::Merge),
            ActionError::MergeBlocked(vec![MergeBlocker::AlreadyMerged]),
        );
        fails(
            &mut w,
            Action::SetVerdict {
                review: 91,
                verdict: Verdict::Approve,
            },
            ActionError::Merged(91),
        );
    }

    #[test]
    fn merging_a_pull_request_without_ticket_marks_its_entry() {
        let mut w = ws();
        ready_87(&mut w);
        ok(&mut w, merge(87, MergeMethod::Squash), 1);
        let item = w.item("pr87").unwrap();
        assert_eq!(item.pr.as_ref().unwrap().label(), "#87 · gemergt");
        assert_eq!(
            crate::team::state_label(item),
            ("Gemergt".into(), Tone::Indigo)
        );
        assert!(item.read);
        assert_eq!(item.reason_text, "Gemergt (Squash)");
        let entry = w.log.last().unwrap();
        assert_eq!(
            (entry.actor, &entry.target),
            (Actor::Du, &Some(Target::Review(87)))
        );
    }

    #[test]
    fn a_failed_check_can_be_restarted_and_turns_green_after_a_few_ticks() {
        let mut w = ws();
        assert_eq!(w.review(85).unwrap().checks, Checks::Failed);
        fails(
            &mut w,
            Action::RestartCheck {
                review: 85,
                check: "Build".into(),
            },
            ActionError::NothingToDo,
        );
        fails(
            &mut w,
            Action::RestartCheck {
                review: 85,
                check: "Gibt es nicht".into(),
            },
            ActionError::UnknownCheck("Gibt es nicht".into()),
        );
        fails(&mut w, Action::AdvanceChecks, ActionError::NothingToDo);
        ok(
            &mut w,
            Action::RestartCheck {
                review: 85,
                check: "Tests (Browser)".into(),
            },
            1,
        );
        assert_eq!(w.running_checks(), 1);
        for _ in 0..CHECK_STEPS - 1 {
            ok(&mut w, Action::AdvanceChecks, 0);
        }
        let applied = ok(&mut w, Action::AdvanceChecks, 1);
        assert_eq!(applied.summary, "Check „Tests (Browser)“ in #85 bestanden");
        assert_eq!(w.log.last().unwrap().actor, Actor::System);
        assert_eq!(w.running_checks(), 0);
        // Der Lint-Fehler ist echt: Der Gesamtstand bleibt rot.
        assert_eq!(w.review(85).unwrap().checks, Checks::Failed);

        // Nach dem Neustart bleibt Lint rot …
        ok(
            &mut w,
            Action::RestartCheck {
                review: 85,
                check: "Lint".into(),
            },
            1,
        );
        for _ in 0..CHECK_STEPS {
            let _ = w.apply(Action::AdvanceChecks, Actor::System, T);
        }
        assert_eq!(w.review(85).unwrap().checks, Checks::Failed);
        assert!(w.log.last().unwrap().text.contains("wieder fehlgeschlagen"));
    }

    #[test]
    fn green_checks_flow_into_the_inbox_entry() {
        let mut w = ws();
        // Lint „repariert“: Neustart geht grün aus.
        w.reviews
            .iter_mut()
            .find(|r| r.number == 85)
            .unwrap()
            .check_runs[1]
            .rerun = Checks::Passed;
        for check in ["Lint", "Tests (Browser)"] {
            ok(
                &mut w,
                Action::RestartCheck {
                    review: 85,
                    check: check.into(),
                },
                1,
            );
        }
        assert_eq!(
            w.item("pr85").unwrap().pr.as_ref().unwrap().checks,
            Checks::Running
        );
        for _ in 0..CHECK_STEPS {
            let _ = w.apply(Action::AdvanceChecks, Actor::System, T);
        }
        assert_eq!(w.review(85).unwrap().checks, Checks::Passed);
        let item = w.item("pr85").unwrap();
        assert_eq!(item.pr.as_ref().unwrap().checks, Checks::Passed);
        assert_eq!(item.reason, ReasonKind::ReviewRequested);
        // Jetzt fehlt nur noch das gesendete Review.
        assert_eq!(w.merge_blockers(85), vec![MergeBlocker::ReviewNotSent]);
    }

    #[test]
    fn line_comment_suggestions_go_through_the_actions() {
        let mut w = ws();
        let file = w.review(87).unwrap().files[0].name.clone();
        // Position 8: `  background: #ffffff;` (neue Zeile).
        ok(
            &mut w,
            Action::AddLineComment {
                review: 87,
                file: file.clone(),
                line: 5,
                text: String::new(),
                suggestion: Some("  background: var(--comp-slider-thumb);".into()),
            },
            1,
        );
        let comment = w.review(87).unwrap().line_comments[0].clone();
        assert!(
            w.log
                .last()
                .unwrap()
                .text
                .contains("mit Änderungsvorschlag")
        );
        assert!(w.review(87).unwrap().suggestion_diff(&comment).is_some());
        fails(
            &mut w,
            Action::EditLineComment {
                review: 87,
                comment: comment.id.clone(),
                text: String::new(),
                suggestion: None,
            },
            ActionError::InvalidInput,
        );
        ok(
            &mut w,
            Action::EditLineComment {
                review: 87,
                comment: comment.id.clone(),
                text: "Besser so".into(),
                suggestion: None,
            },
            1,
        );
        assert_eq!(w.review(87).unwrap().line_comments[0].suggestion, None);
    }

    #[test]
    fn new_actions_survive_json_and_old_ones_still_parse() {
        for a in [
            merge(87, MergeMethod::Squash),
            Action::RestartCheck {
                review: 85,
                check: "Lint".into(),
            },
            Action::AdvanceChecks,
        ] {
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
        }
        let old = r#"{"AddLineComment":{"review":1,"file":"a","line":0,"text":"x"}}"#;
        assert!(matches!(
            serde_json::from_str::<Action>(old).unwrap(),
            Action::AddLineComment {
                suggestion: None,
                ..
            }
        ));
    }

    #[test]
    fn a_merged_pull_request_survives_saving() {
        let mut w = ws();
        ready_87(&mut w);
        ok(&mut w, merge(87, MergeMethod::Squash), 1);
        let (back, _) = persist::restore(Some(&persist::save(&w)));
        assert_eq!(back, w);
    }

    // ---------- Tickets bearbeiten ----------

    fn edit(item: &str, title: &str, description: &str) -> Action {
        Action::EditTicket {
            item: item.into(),
            title: title.into(),
            description: description.into(),
        }
    }

    #[test]
    fn editing_a_ticket_changes_title_and_description() {
        let mut w = ws();
        let applied = ok(
            &mut w,
            edit("bau150", "  Sheet: Detents  ", "Neuer Text"),
            1,
        );
        assert_eq!(
            applied.summary,
            "Titel und Beschreibung von BAU-150 geändert"
        );
        let item = w.item("bau150").unwrap();
        assert_eq!(
            item.title, "Sheet: Detents",
            "Titel ohne Leerraum an den Rändern"
        );
        assert_eq!(item.description, "Neuer Text");
        let entry = w.log.last().unwrap();
        assert_eq!(
            (entry.actor, &entry.target),
            (Actor::Du, &Some(Target::Item("bau150".into())))
        );

        let applied = ok(&mut w, edit("bau150", "Sheet: Detents", "Anderer Text"), 1);
        assert_eq!(applied.summary, "Beschreibung von BAU-150 geändert");
        let applied = ok(&mut w, edit("bau150", "Sheet", "Anderer Text"), 1);
        assert_eq!(applied.summary, "Titel von BAU-150 geändert");
        // Eine leere Beschreibung ist erlaubt.
        ok(&mut w, edit("bau150", "Sheet", ""), 1);
        assert_eq!(w.item("bau150").unwrap().description, "");
    }

    #[test]
    fn a_ticket_needs_a_title() {
        let mut w = ws();
        for title in ["", "   ", "\n\t"] {
            fails(
                &mut w,
                edit("bau150", title, "Text"),
                ActionError::InvalidInput,
            );
        }
        fails(
            &mut w,
            edit(
                "bau150",
                "Sheet: Detents mittel und groß",
                "Das Sheet soll zwei Höhen unterstützen (mittel, groß) und sich per Grabber zwischen ihnen ziehen lassen.",
            ),
            ActionError::Unchanged,
        );
        fails(&mut w, edit("pr87", "Neu", "Neu"), ActionError::NotATicket);
        fails(
            &mut w,
            edit("gibt-es-nicht", "Neu", ""),
            ActionError::UnknownItem("gibt-es-nicht".into()),
        );
    }

    fn assign(item: &str, person: Person) -> Action {
        Action::AssignTicket {
            item: item.into(),
            person,
        }
    }

    fn lena() -> Person {
        Person::new("Lena", "LK")
    }

    #[test]
    fn assigning_to_someone_else_ends_the_assigned_rule() {
        use crate::attention::Rule;
        let mut w = ws();
        let assigned = |w: &Workspace| {
            w.hits(w.item("bau150").unwrap(), T)
                .iter()
                .any(|h| h.rule == Rule::Assigned)
        };
        assert!(assigned(&w), "BAU-150 ist mir zugewiesen");
        assert!(w.visible(Filter::Mine).iter().any(|i| i.id == "bau150"));

        let applied = ok(&mut w, assign("bau150", lena()), 1);
        assert_eq!(applied.summary, "BAU-150 an Lena zugewiesen");
        let item = w.item("bau150").unwrap();
        assert_eq!((item.owner.clone(), item.scope), (lena(), Scope::Team));
        assert!(!assigned(&w), "die Regel endet");
        assert!(!w.visible(Filter::Mine).iter().any(|i| i.id == "bau150"));
        assert!(w.visible(Filter::Team).iter().any(|i| i.id == "bau150"));
        // Auch die Auslastung wandert mit.
        let load = |w: &Workspace, name: &str| {
            crate::team::members(w)
                .into_iter()
                .find(|m| m.person.name == name)
                .map_or(0, |m| m.total())
        };
        assert_eq!(load(&w, "Lena"), 1);
    }

    #[test]
    fn assigning_to_me_triggers_the_assigned_rule() {
        use crate::attention::Rule;
        let mut w = ws();
        ok(&mut w, assign("bau150", lena()), 1);
        // Das Fälligkeitsdatum ist vorbei: ohne die Überfälligkeit bleibt nur die Zuweisung.
        w.items.iter_mut().find(|i| i.id == "bau150").unwrap().due = None;
        w.mark_done("bau150");
        assert!(!w.needs_attention(w.item("bau150").unwrap(), T));

        let applied = ok(&mut w, assign("bau150", Person::me()), 1);
        assert_eq!(applied.summary, "BAU-150 dir zugewiesen");
        let item = w.item("bau150").unwrap();
        assert_eq!(
            (item.owner.clone(), item.scope),
            (Person::me(), Scope::Mine)
        );
        assert!(!item.read, "neu zugewiesen heißt wieder ungelesen");
        let hits = w.hits(item, T);
        assert_eq!(
            hits.first().map(|h| (h.rule, h.text.as_str())),
            Some((Rule::Assigned, "Dir zugewiesen"))
        );
        assert!(
            w.attention(Filter::Mine, T)
                .iter()
                .any(|i| i.id == "bau150")
        );
        // Ist die Regel ausgeschaltet, zählt die Zuweisung nicht.
        w.set_rule(Rule::Assigned, false);
        assert!(!w.needs_attention(w.item("bau150").unwrap(), T));
    }

    #[test]
    fn assignment_checks_person_and_kind() {
        let mut w = ws();
        fails(
            &mut w,
            assign("bau150", Person::new("Fremde", "FR")),
            ActionError::UnknownPerson("Fremde".into()),
        );
        // Der Agent bekommt Tickets über seinen Lauf, nicht über die Zuweisung.
        fails(
            &mut w,
            assign("bau150", Person::agent()),
            ActionError::UnknownPerson("Agent".into()),
        );
        fails(
            &mut w,
            assign("bau150", Person::me()),
            ActionError::Unchanged,
        );
        fails(
            &mut w,
            assign("pr87", Person::me()),
            ActionError::NotATicket,
        );
        fails(
            &mut w,
            assign("nix", Person::me()),
            ActionError::UnknownItem("nix".into()),
        );
        let names: Vec<String> = w.people().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["Du", "Jonas", "Mira", "Lena"]);
    }

    #[test]
    fn priority_is_set_logged_and_defaults_to_medium() {
        let mut w = ws();
        assert_eq!(w.item("bau150").unwrap().priority, Priority::Medium);
        let applied = ok(
            &mut w,
            Action::SetPriority {
                item: "bau150".into(),
                priority: Priority::Highest,
            },
            1,
        );
        assert_eq!(
            applied.summary,
            "Priorität von BAU-150 auf „Höchste“ gesetzt"
        );
        assert_eq!(w.item("bau150").unwrap().priority, Priority::Highest);
        let same = Action::SetPriority {
            item: "bau150".into(),
            priority: Priority::Highest,
        };
        fails(&mut w, same, ActionError::Unchanged);
        let pr = Action::SetPriority {
            item: "pr87".into(),
            priority: Priority::Low,
        };
        fails(&mut w, pr, ActionError::NotATicket);
    }

    #[test]
    fn old_saves_without_the_new_fields_still_load() {
        let mut w = ws();
        w.apply(
            Action::SetPriority {
                item: "bau150".into(),
                priority: Priority::High,
            },
            Actor::Du,
            T,
        )
        .unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&persist::save(&w)).unwrap();
        let ws_json = value.get_mut("workspace").unwrap().as_object_mut().unwrap();
        for key in ["unlinked", "pr_status_off", "status_seen"] {
            assert!(ws_json.remove(key).is_some(), "{key} wird gespeichert");
        }
        for item in ws_json["items"].as_array_mut().unwrap() {
            let item = item.as_object_mut().unwrap();
            item.remove("priority");
            if let Some(pr) = item.get_mut("pr").and_then(|p| p.as_object_mut()) {
                pr.remove("review_requested");
            }
        }
        let (back, restored) = persist::restore(Some(&value.to_string()));
        assert!(restored, "alter Stand lädt");
        assert_eq!(back.item("bau150").unwrap().priority, Priority::Medium);
        assert!(!back.pr_status_off && back.unlinked.is_empty());
        assert!(
            !back
                .item("bau138")
                .unwrap()
                .pr
                .as_ref()
                .unwrap()
                .review_requested
        );
    }

    // ---------- Ticket und Pull Request ----------

    fn link(item: &str, pr: u32) -> Action {
        Action::LinkPr {
            item: item.into(),
            pr,
        }
    }

    fn unlink(item: &str) -> Action {
        Action::UnlinkPr { item: item.into() }
    }

    fn status(w: &Workspace, id: &str) -> TicketStatus {
        w.item(id).unwrap().status
    }

    #[test]
    fn linking_by_hand_connects_both_sides() {
        let mut w = ws();
        // Status nicht mitziehen, hier geht es nur um die Verknüpfung.
        w.pr_status_off = true;
        let applied = ok(&mut w, link("bau150", 85), 1);
        assert_eq!(applied.summary, "BAU-150 mit #85 verknüpft");
        let ticket = w.item("bau150").unwrap();
        assert_eq!(ticket.pr.as_ref().map(|p| p.number), Some(85));
        assert_eq!(
            ticket.branch.as_deref(),
            Some("feature/BAU-127-list-separator")
        );
        assert_eq!(w.review(85).unwrap().ticket_key.as_deref(), Some("BAU-150"));
        assert_eq!(w.ticket_of_pr(85).map(|t| t.id.as_str()), Some("bau150"));
        // Die Kopie am Ticket entspricht dem Eintrag des Pull Requests (Checks rot).
        assert_eq!(ticket.pr.as_ref().unwrap().checks, Checks::Failed);

        fails(&mut w, link("bau150", 85), ActionError::Unchanged);
        fails(
            &mut w,
            link("bau150", 87),
            ActionError::Link(LinkError::TicketHasPr(85)),
        );
        fails(
            &mut w,
            link("bau142", 85),
            ActionError::Link(LinkError::PrHasTicket("BAU-150".into())),
        );
        fails(
            &mut w,
            link("bau142", 999),
            ActionError::Link(LinkError::UnknownPr(999)),
        );
        fails(&mut w, link("pr87", 85), ActionError::NotATicket);
        fails(
            &mut w,
            link("nix", 85),
            ActionError::UnknownItem("nix".into()),
        );
    }

    #[test]
    fn unlinking_removes_both_sides_and_the_branch_it_brought() {
        let mut w = ws();
        w.pr_status_off = true;
        ok(&mut w, link("bau150", 85), 1);
        let applied = ok(&mut w, unlink("bau150"), 1);
        assert_eq!(applied.summary, "Verknüpfung von BAU-150 mit #85 gelöst");
        let ticket = w.item("bau150").unwrap();
        assert_eq!((ticket.pr.clone(), ticket.branch.clone()), (None, None));
        assert_eq!(w.review(85).unwrap().ticket_key, None);
        assert!(w.ticket_of_pr(85).is_none());
        fails(&mut w, unlink("bau150"), ActionError::NothingToDo);
        fails(&mut w, unlink("pr87"), ActionError::NotATicket);
        // Ein eigener Branch des Tickets bleibt.
        w.items
            .iter_mut()
            .find(|i| i.id == "bau150")
            .unwrap()
            .branch = Some("mein-branch".into());
        ok(&mut w, link("bau150", 85), 1);
        ok(&mut w, unlink("bau150"), 1);
        assert_eq!(
            w.item("bau150").unwrap().branch.as_deref(),
            Some("mein-branch")
        );
    }

    fn name_pr85(w: &mut Workspace, branch: &str, title: &str) {
        let review = w.reviews.iter_mut().find(|r| r.number == 85).unwrap();
        review.branch = branch.into();
        review.title = title.into();
        review.ticket_key = None;
    }

    #[test]
    fn the_branch_or_title_links_automatically() {
        let mut w = ws();
        w.pr_status_off = true;
        name_pr85(&mut w, "feature/bau-150-sheet", "Sheet");
        assert_eq!(w.auto_links(), [("bau150".to_string(), 85)]);
        let applied = ok(&mut w, Action::AutoLinkPrs, 1);
        assert_eq!(applied.summary, "BAU-150 automatisch mit #85 verknüpft");
        let entry = w.log.last().unwrap();
        assert_eq!(
            entry.actor,
            Actor::System,
            "die Automatik handelt als System"
        );
        assert_eq!(
            w.item("bau150").unwrap().pr.as_ref().map(|p| p.number),
            Some(85)
        );
        assert_eq!(w.review(85).unwrap().ticket_key.as_deref(), Some("BAU-150"));
        // Ein zweiter Lauf findet nichts mehr.
        fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);

        // Den Titel zählt es ebenso.
        let mut w = ws();
        w.pr_status_off = true;
        name_pr85(&mut w, "mira/lint-fix", "BAU-150: Sheet");
        ok(&mut w, Action::AutoLinkPrs, 1);
        assert_eq!(w.review(85).unwrap().ticket_key.as_deref(), Some("BAU-150"));
    }

    #[test]
    fn auto_linking_needs_the_whole_key() {
        let mut w = ws();
        name_pr85(&mut w, "feature/bau-1500-sheet", "Sheet");
        fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
        name_pr85(&mut w, "feature/xbau-150", "Sheet 150");
        fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
        // Ein Pull Request, der schon zu einem vorhandenen Ticket gehört, bleibt dort.
        name_pr85(&mut w, "feature/bau-150-sheet", "Sheet");
        w.reviews
            .iter_mut()
            .find(|r| r.number == 85)
            .unwrap()
            .ticket_key = Some("BAU-142".into());
        fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
        // Und ein Ticket mit eigenem Pull Request nimmt keinen zweiten.
        w.reviews
            .iter_mut()
            .find(|r| r.number == 85)
            .unwrap()
            .ticket_key = None;
        w.items.iter_mut().find(|i| i.id == "bau150").unwrap().pr =
            w.item("bau138").unwrap().pr.clone();
        fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
    }

    #[test]
    fn a_link_removed_by_hand_is_not_set_again() {
        let mut w = ws();
        w.pr_status_off = true;
        name_pr85(&mut w, "feature/bau-150-sheet", "Sheet");
        ok(&mut w, Action::AutoLinkPrs, 1);
        ok(&mut w, unlink("bau150"), 1);
        assert_eq!(w.unlinked, [(85, "BAU-150".to_string())]);
        fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
        // Von Hand verknüpfen geht weiter und hebt die Sperre auf.
        ok(&mut w, link("bau150", 85), 1);
        assert!(w.unlinked.is_empty());
        // Die Sperre überlebt das Speichern.
        ok(&mut w, unlink("bau150"), 1);
        let (back, _) = persist::restore(Some(&persist::save(&w)));
        assert_eq!(back.unlinked, w.unlinked);
    }

    // ---------- Status folgt dem Pull Request ----------

    #[test]
    fn the_status_follows_the_pull_request() {
        let mut w = ws();
        // Entwurf, kein Review angefragt → In Arbeit.
        assert_eq!(status(&w, "bau150"), TicketStatus::Open);
        let applied = ok(&mut w, link("bau150", 85), 2);
        assert_eq!(applied.summary, "BAU-150 mit #85 verknüpft");
        assert_eq!(status(&w, "bau150"), TicketStatus::InProgress);
        let entry = w.log.last().unwrap();
        assert_eq!(entry.actor, Actor::System);
        assert_eq!(
            entry.text,
            "Status von BAU-150 automatisch auf „In Arbeit“ gesetzt – #85 ist noch in Arbeit"
        );

        // Review angefragt → Review.
        let applied = ok(&mut w, Action::RequestReview { review: 85 }, 2);
        assert_eq!(applied.summary, "Review für #85 angefragt");
        assert_eq!(status(&w, "bau150"), TicketStatus::Review);
        let entry = w.log.last().unwrap();
        assert_eq!(
            (entry.actor, entry.text.as_str()),
            (
                Actor::System,
                "Status von BAU-150 automatisch auf „Review“ gesetzt – für #85 ist ein Review angefragt"
            )
        );
        fails(
            &mut w,
            Action::RequestReview { review: 85 },
            ActionError::Unchanged,
        );
        fails(
            &mut w,
            Action::RequestReview { review: 999 },
            ActionError::UnknownReview(999),
        );

        // Nichts Neues: der Takt tut nichts.
        fails(&mut w, Action::SyncTicketStatus, ActionError::NothingToDo);
    }

    #[test]
    fn merging_finishes_the_linked_ticket_through_the_same_path() {
        let mut w = ws();
        // #91 gehört zu BAU-138.
        w.items
            .iter_mut()
            .find(|i| i.id == "bau138")
            .unwrap()
            .pr
            .as_mut()
            .unwrap()
            .draft = false;
        ok(
            &mut w,
            Action::SendReview {
                review: 91,
                comment: String::new(),
            },
            1,
        );
        assert_eq!(status(&w, "bau138"), TicketStatus::Review);
        ok(&mut w, merge(91, MergeMethod::Squash), 2);
        let ticket = w.item("bau138").unwrap();
        assert_eq!(ticket.status, TicketStatus::Done);
        assert!(ticket.read && ticket.agent_note.is_none());
        let entry = w.log.last().unwrap();
        assert_eq!(entry.actor, Actor::System);
        assert!(entry.text.ends_with("– #91 ist gemergt"));
    }

    #[test]
    fn a_status_set_by_hand_stays_until_the_pull_request_moves() {
        let mut w = ws();
        ok(&mut w, link("bau150", 85), 2);
        // Von Hand zurück auf „Offen“: Die Automatik überstimmt das nicht.
        ok(
            &mut w,
            Action::SetStatus {
                item: "bau150".into(),
                status: TicketStatus::Open,
            },
            1,
        );
        fails(&mut w, Action::SyncTicketStatus, ActionError::NothingToDo);
        assert_eq!(status(&w, "bau150"), TicketStatus::Open);
        // Bewegt sich der Pull Request, zieht das Ticket wieder nach.
        ok(&mut w, Action::RequestReview { review: 85 }, 2);
        assert_eq!(status(&w, "bau150"), TicketStatus::Review);
    }

    #[test]
    fn a_jira_project_narrows_the_inbox() {
        use crate::{Filter, SourceFilter};
        let mut w = ws();
        let mut fremd = w.item_by_key("BAU-142").unwrap().clone();
        fremd.id = "web7".into();
        fremd.key = "WEB-7".into();
        w.items.push(fremd);
        assert_eq!(w.projects(), ["BAU", "WEB"]);
        let alle = w.search(Filter::All, SourceFilter::All, "", 0).len();

        let applied = ok(
            &mut w,
            Action::SelectJiraProject {
                project: Some(" web ".into()),
            },
            1,
        );
        assert_eq!(applied.summary, "Jira-Projekt gewählt: WEB");
        assert_eq!(w.jira_project.as_deref(), Some("WEB"));
        let treffer = w.search(Filter::All, SourceFilter::All, "", 0);
        assert!(treffer.iter().any(|i| i.key == "WEB-7"));
        assert!(treffer.iter().all(|i| i.project() != Some("BAU")));
        // Pull Requests gehören zu keinem Projekt und bleiben sichtbar.
        assert!(treffer.iter().any(|i| !i.is_ticket()));
        fails(
            &mut w,
            Action::SelectJiraProject {
                project: Some("WEB".into()),
            },
            ActionError::Unchanged,
        );
        fails(
            &mut w,
            Action::SelectJiraProject {
                project: Some("A) OR (B".into()),
            },
            ActionError::InvalidInput,
        );

        // Die Wahl ist eine Einstellung: Sie übersteht das Zurücksetzen der Demo.
        w.apply(Action::ResetDemo, Actor::Du, T).unwrap();
        assert_eq!(w.jira_project.as_deref(), Some("WEB"));
        assert_eq!(w.projects(), ["BAU", "WEB"]);

        let applied = ok(&mut w, Action::SelectJiraProject { project: None }, 1);
        assert_eq!(applied.summary, "Jira-Projekt: alle");
        assert!(w.search(Filter::All, SourceFilter::All, "", 0).len() < alle);
    }

    #[test]
    fn the_automation_can_be_switched_off() {
        let mut w = ws();
        let off = Action::SetStatusFollowsPr { enabled: false };
        let on = Action::SetStatusFollowsPr { enabled: true };
        let applied = ok(&mut w, off.clone(), 1);
        assert_eq!(
            applied.summary,
            "Status folgt dem Pull Request: ausgeschaltet"
        );
        fails(&mut w, off, ActionError::Unchanged);

        // Aus: Verknüpfen, Review anfragen und Mergen lassen den Status in Ruhe.
        ok(&mut w, link("bau150", 85), 1);
        ok(&mut w, Action::RequestReview { review: 85 }, 1);
        assert_eq!(status(&w, "bau150"), TicketStatus::Open);
        fails(&mut w, Action::SyncTicketStatus, ActionError::NothingToDo);
        w.items
            .iter_mut()
            .find(|i| i.id == "bau138")
            .unwrap()
            .pr
            .as_mut()
            .unwrap()
            .draft = false;
        ok(
            &mut w,
            Action::SendReview {
                review: 91,
                comment: String::new(),
            },
            1,
        );
        ok(&mut w, merge(91, MergeMethod::Merge), 1);
        assert_eq!(
            status(&w, "bau138"),
            TicketStatus::Review,
            "der Pull Request ist gemergt, das Ticket bleibt"
        );
        assert!(w.item("bau138").unwrap().pr.as_ref().unwrap().merged);

        // Die Einstellung übersteht Zurücksetzen und Speichern.
        let (back, _) = persist::restore(Some(&persist::save(&w)));
        assert!(back.pr_status_off);
        w.apply(Action::ResetDemo, Actor::Du, T).unwrap();
        assert!(w.pr_status_off);

        // Wieder an: Was sich bewegt hat, wird nachgeführt, mit dem System als Urheber.
        let mut w = back;
        let applied = ok(&mut w, on, 3);
        assert_eq!(
            applied.summary,
            "Status folgt dem Pull Request: eingeschaltet"
        );
        assert_eq!(status(&w, "bau150"), TicketStatus::Review);
        assert_eq!(status(&w, "bau138"), TicketStatus::Done);
        assert!(w.log.iter().rev().take(2).all(|e| e.actor == Actor::System));
    }

    #[test]
    fn the_agent_publishes_through_the_same_status_rule() {
        let mut w = ws();
        w.apply(
            Action::StartAgent {
                ticket: "BAU-150".into(),
                options: direct(),
            },
            Actor::Du,
            T,
        )
        .unwrap();
        for _ in 0..STEPS {
            let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T);
        }
        let ticket = w.item("bau150").unwrap();
        assert_eq!(ticket.status, TicketStatus::Review);
        assert!(ticket.pr.as_ref().unwrap().review_requested);
        // Der Agent hat das Ticket verknüpft; der Takt findet nichts mehr zu tun.
        assert!(w.ticket_of_pr(92).is_some());
        fails(&mut w, Action::SyncTicketStatus, ActionError::NothingToDo);
        fails(&mut w, Action::AutoLinkPrs, ActionError::NothingToDo);
    }

    #[test]
    fn only_you_merge_even_with_the_automation() {
        let mut w = ws();
        for actor in [Actor::Agent, Actor::System] {
            let before = w.clone();
            assert_eq!(
                w.apply(merge(87, MergeMethod::Merge), actor, T),
                Err(ActionError::MergeNotAllowed(actor))
            );
            assert_eq!(w, before);
        }
        // Die neuen Aktionen mergen nie.
        ok(&mut w, link("bau150", 85), 2);
        let _ = w.apply(Action::SyncTicketStatus, Actor::System, T);
        assert!(w.reviews.iter().all(|r| r.merged.is_none()));
    }

    #[test]
    fn the_new_ticket_and_link_actions_survive_json() {
        for a in [
            edit("bau150", "Titel", "Text"),
            assign("bau150", lena()),
            Action::SetPriority {
                item: "bau150".into(),
                priority: Priority::Lowest,
            },
            link("bau150", 85),
            unlink("bau150"),
            Action::AutoLinkPrs,
            Action::RequestReview { review: 85 },
            Action::SyncTicketStatus,
            Action::SetStatusFollowsPr { enabled: false },
        ] {
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
        }
    }

    // ---------- Folgeaufträge ----------

    fn run_to_end(w: &mut Workspace) {
        for _ in 0..STEPS * 2 {
            // Rückfragen beantwortet der Test mit dem ersten Vorschlag.
            let asking = w
                .runs
                .iter()
                .find(|r| r.is_asking())
                .map(|r| (r.key(), r.question.as_ref().unwrap().suggestions[0].clone()));
            if let Some((run, text)) = asking {
                w.apply(Action::AnswerAgent { run, text }, Actor::Du, T)
                    .unwrap();
            }
            if w.running_agents() == 0 {
                break;
            }
            w.apply(Action::AdvanceAgents, Actor::Agent, T).unwrap();
        }
    }

    fn address(review: u32, ids: &[&str], options: AgentOptions) -> Action {
        Action::AddressReview {
            review,
            comments: ids.iter().map(|s| s.to_string()).collect(),
            options,
        }
    }

    #[test]
    fn address_review_pushes_to_the_same_branch_and_resolves_the_comments() {
        let mut w = ws();
        let prs = w.next_pr_number();
        let applied = ok(&mut w, address(91, &["c1", "c2"], direct()), 1);
        assert!(
            applied.summary.contains("2 Kommentare"),
            "{}",
            applied.summary
        );
        let run = w.run("review-91").unwrap();
        assert!(run.is_running() && run.is_followup());
        assert_eq!(run.branch(), "agent/BAU-138-segmented-picker");
        assert_eq!(run.files, ["controls.rs", "components.css"]);
        assert_eq!(w.next_pr_number(), prs, "kein neuer Pull Request");
        // Das Ticket mit seinem Lauf „Ticket umsetzen“ bleibt unberührt.
        assert!(w.run("BAU-138").unwrap().is_done());

        run_to_end(&mut w);
        let run = w.run("review-91").unwrap();
        assert!(run.is_done());
        let review = w.review(91).unwrap();
        assert!(review.open_comments().is_empty());
        assert!(
            review
                .line_comments
                .iter()
                .all(|c| c.resolved.as_deref() == Some("review-91"))
        );
        // Der Änderungsvorschlag aus c1 steht jetzt im Diff.
        let file = review
            .files
            .iter()
            .find(|f| f.name == "controls.rs")
            .unwrap();
        assert!(
            file.lines
                .iter()
                .any(|l| l.text.contains("select_with_keys"))
        );
        assert!(!review.sent && review.verdict == Verdict::Comment);
        assert_eq!(w.reviews.len(), 3);
        // Der Eintrag zeigt den neuen Commit und steht wieder auf „Review angefragt“.
        let item = w.item_by_key("BAU-138").unwrap();
        assert_eq!(item.reason, crate::model::ReasonKind::ReviewRequested);
        assert!(
            item.agent_note
                .as_deref()
                .unwrap()
                .starts_with("Neuer Commit")
        );
        assert!(!item.pr.as_ref().unwrap().changes_requested);
        assert!(!item.read);
        assert!(item.comments.last().unwrap().author.is_agent());
        // Protokoll: Auftrag, Schritte, Ergebnis am Review.
        let texts: Vec<&str> = w.log.iter().map(|e| e.text.as_str()).collect();
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Kommentare in #91: Branch ausgecheckt"))
        );
        assert!(texts.last().unwrap().contains("auf den Branch gepusht"));
    }

    #[test]
    fn address_review_takes_a_selection_and_rejects_bad_ones() {
        let mut w = ws();
        fails(
            &mut w,
            address(91, &[], direct()),
            ActionError::Agent(AgentError::NoComments),
        );
        fails(
            &mut w,
            address(91, &["x9"], direct()),
            ActionError::UnknownComment("x9".into()),
        );
        fails(
            &mut w,
            address(5, &["c1"], direct()),
            ActionError::UnknownReview(5),
        );
        ok(&mut w, address(91, &["c2"], direct()), 1);
        fails(
            &mut w,
            address(91, &["c1"], direct()),
            ActionError::Agent(AgentError::AlreadyRunning),
        );
        run_to_end(&mut w);
        // Nur c2 ist erledigt; c1 bleibt offen, und c2 lässt sich nicht noch einmal umsetzen.
        let r = w.review(91).unwrap().clone();
        assert_eq!(r.open_comments().len(), 1);
        assert!(r.open_comments()[0].id == "c1");
        fails(
            &mut w,
            address(91, &["c2"], direct()),
            ActionError::Agent(AgentError::NoComments),
        );
        // Das Ergebnis der ersten Runde blieb, ohne dass c1 verändert wurde.
        assert!(
            r.files
                .iter()
                .all(|f| f.lines.iter().all(|l| !l.text.contains("select_with_keys")))
        );
    }

    #[test]
    fn address_review_waits_for_plan_approval_and_can_be_steered() {
        let mut w = ws();
        ok(&mut w, address(91, &["c1"], AgentOptions::default()), 2);
        let run = w.run("review-91").unwrap();
        assert!(run.is_awaiting());
        assert!(
            run.plan()
                .unwrap()
                .steps
                .iter()
                .any(|s| s.contains("kein neuer Pull Request"))
        );
        assert_eq!(w.waiting_agents(), 1);
        // Nichts geschieht vor der Freigabe.
        fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
        assert!(w.review(91).unwrap().open_comments().len() == 2);
        ok(
            &mut w,
            Action::ApprovePlan {
                ticket: "review-91".into(),
            },
            1,
        );
        ok(
            &mut w,
            Action::SteerAgent {
                run: "review-91".into(),
                text: "Mit Tests".into(),
            },
            1,
        );
        ok(
            &mut w,
            Action::PauseAgent {
                run: "review-91".into(),
            },
            1,
        );
        fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
        ok(
            &mut w,
            Action::ResumeAgent {
                run: "review-91".into(),
            },
            1,
        );
        // Rückfrage nach dem zweiten Schritt, mit Antwort geht es weiter.
        // Der erste Takt bestätigt zusätzlich den Hinweis.
        ok(&mut w, Action::AdvanceAgents, 2);
        ok(&mut w, Action::AdvanceAgents, 1);
        let step = w.apply(Action::AdvanceAgents, Actor::Agent, T).unwrap();
        assert!(step.logged >= 1);
        assert!(w.run("review-91").unwrap().is_asking());
        ok(
            &mut w,
            Action::AnswerAgent {
                run: "review-91".into(),
                text: "Nein".into(),
            },
            1,
        );
        run_to_end(&mut w);
        assert!(w.run("review-91").unwrap().is_done());
        // Ablehnen räumt den Folgeauftrag ganz weg.
        let mut w = ws();
        ok(&mut w, address(91, &["c1"], AgentOptions::default()), 2);
        ok(
            &mut w,
            Action::RejectPlan {
                ticket: "review-91".into(),
            },
            1,
        );
        assert!(w.run("review-91").is_none());
        assert_eq!(w.review(91).unwrap().open_comments().len(), 2);
    }

    #[test]
    fn foreign_comments_stay_out_of_my_review_and_cannot_be_edited() {
        let mut w = ws();
        let r = w.review(91).unwrap();
        assert_eq!(
            r.comment_count(),
            0,
            "fremde Kommentare gehen nicht mit hinaus"
        );
        fails(
            &mut w,
            Action::EditLineComment {
                review: 91,
                comment: "c1".into(),
                text: "Anders".into(),
                suggestion: None,
            },
            ActionError::InvalidInput,
        );
        fails(
            &mut w,
            Action::DeleteLineComment {
                review: 91,
                comment: "c1".into(),
            },
            ActionError::UnknownComment("c1".into()),
        );
        // Eigene, umgesetzte Kommentare gehen nicht noch einmal hinaus.
        let mut r = w.review(87).unwrap().clone();
        let id = r
            .add_line_comment("tokens.css", 1, "Eigener", None)
            .unwrap();
        assert_eq!(r.comment_count(), 1);
        r.resolve_comments(&[id], "review-87");
        assert_eq!(r.comment_count(), 0);
    }

    fn investigate_85(w: &mut Workspace, check: &str) {
        ok(
            w,
            Action::InvestigateCheck {
                review: 85,
                check: check.into(),
            },
            1,
        );
    }

    #[test]
    fn a_red_check_is_diagnosed_and_nothing_changes_before_you_accept() {
        let mut w = ws();
        investigate_85(&mut w, "Lint");
        let key = "check-85-lint";
        assert!(w.run(key).unwrap().is_running());
        let branch_before = w.review(85).unwrap().files.clone();
        run_to_end(&mut w);
        let run = w.run(key).unwrap();
        assert!(run.is_diagnosed(), "wartet auf die Entscheidung");
        let d = run.diagnosis.clone().unwrap();
        assert_eq!(d.location().as_deref(), Some("list.rs · Zeile 62"));
        assert!(d.patch.is_some());
        assert_eq!(
            w.review(85).unwrap().files,
            branch_before,
            "Branch unberührt"
        );
        assert_eq!(w.review(85).unwrap().checks, Checks::Failed);
        assert_eq!(w.waiting_agents(), 1);
        // Weiter geht es nicht von allein.
        fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
        // Aufmerksamkeit: Die Diagnose steht oben bei „hängt“.
        let stuck = crate::team::stuck(&w);
        let entry = stuck.iter().find(|s| s.key == "#85").unwrap();
        assert_eq!(entry.kind, crate::team::StuckKind::DiagnosisWaiting);
        assert_eq!(entry.diagnosis.as_deref(), Some(key));

        ok(&mut w, Action::ApplyFix { run: key.into() }, 1);
        run_to_end(&mut w);
        assert!(w.run(key).unwrap().is_done());
        let review = w.review(85).unwrap();
        let list = review.files.iter().find(|f| f.name == "list.rs").unwrap();
        assert!(list.lines.iter().all(|l| !l.text.contains("let inset")));
        let lint = review.check_runs.iter().find(|c| c.name == "Lint").unwrap();
        assert_eq!(lint.status, Checks::Running, "Check neu gestartet");
        // Der Takt der Checks macht ihn grün; der Browser-Test bleibt rot.
        while w.running_checks() > 0 {
            w.apply(Action::AdvanceChecks, Actor::System, T).unwrap();
        }
        let review = w.review(85).unwrap();
        assert_eq!(
            review
                .check_runs
                .iter()
                .find(|c| c.name == "Lint")
                .unwrap()
                .status,
            Checks::Passed
        );
        assert_eq!(
            review.checks,
            Checks::Failed,
            "Tests (Browser) ist weiter rot"
        );
        assert!(
            w.item_by_key("#85")
                .unwrap()
                .comments
                .last()
                .unwrap()
                .text
                .contains("gepusht")
        );
    }

    #[test]
    fn discarding_a_diagnosis_ends_the_run_without_touching_the_branch() {
        let mut w = ws();
        investigate_85(&mut w, "Lint");
        run_to_end(&mut w);
        let before = w.review(85).unwrap().clone();
        ok(
            &mut w,
            Action::DiscardFix {
                run: "check-85-lint".into(),
            },
            1,
        );
        assert!(w.run("check-85-lint").is_none());
        assert_eq!(*w.review(85).unwrap(), before);
        fails(
            &mut w,
            Action::ApplyFix {
                run: "check-85-lint".into(),
            },
            ActionError::NothingToDo,
        );
        // Danach lässt sich neu untersuchen.
        investigate_85(&mut w, "Lint");
    }

    #[test]
    fn a_flaky_check_is_only_restarted_and_turns_green() {
        let mut w = ws();
        investigate_85(&mut w, "Tests (Browser)");
        let key = "check-85-tests-browser";
        run_to_end(&mut w);
        assert!(!w.run(key).unwrap().diagnosis.unwrap().changes_code());
        let files = w.review(85).unwrap().files.clone();
        ok(&mut w, Action::ApplyFix { run: key.into() }, 1);
        run_to_end(&mut w);
        assert_eq!(w.review(85).unwrap().files, files);
        while w.running_checks() > 0 {
            w.apply(Action::AdvanceChecks, Actor::System, T).unwrap();
        }
        let r = w.review(85).unwrap();
        assert_eq!(
            r.check_runs
                .iter()
                .find(|c| c.name == "Tests (Browser)")
                .unwrap()
                .status,
            Checks::Passed
        );
    }

    #[test]
    fn only_a_red_check_can_be_investigated() {
        let mut w = ws();
        fails(
            &mut w,
            Action::InvestigateCheck {
                review: 85,
                check: "Build".into(),
            },
            ActionError::Agent(AgentError::CheckNotFailed),
        );
        fails(
            &mut w,
            Action::InvestigateCheck {
                review: 85,
                check: "Nope".into(),
            },
            ActionError::UnknownCheck("Nope".into()),
        );
        fails(
            &mut w,
            Action::InvestigateCheck {
                review: 9,
                check: "Lint".into(),
            },
            ActionError::UnknownReview(9),
        );
        investigate_85(&mut w, "Lint");
        fails(
            &mut w,
            Action::InvestigateCheck {
                review: 85,
                check: "Lint".into(),
            },
            ActionError::Agent(AgentError::AlreadyRunning),
        );
    }

    #[test]
    fn followups_notify_when_waiting_and_when_done() {
        use crate::notify::{NotifyReason, events};
        let mut w = ws();
        investigate_85(&mut w, "Lint");
        let mut waiting = None;
        let mut before = w.clone();
        for _ in 0..STEPS {
            w.apply(Action::AdvanceAgents, Actor::Agent, T).ok();
            let e = events(&before, &w);
            if !e.is_empty() {
                waiting = Some(e);
                break;
            }
            before = w.clone();
        }
        let e = waiting.unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].reason, NotifyReason::AgentWaiting);
        assert!(e[0].title.contains("Check „Lint“ in #85"));
        assert_eq!(e[0].route, "#/agent/check-85-lint");
        ok(
            &mut w,
            Action::ApplyFix {
                run: "check-85-lint".into(),
            },
            1,
        );
        let mut before = w.clone();
        let mut done = Vec::new();
        for _ in 0..STEPS {
            w.apply(Action::AdvanceAgents, Actor::Agent, T).ok();
            done.extend(events(&before, &w));
            before = w.clone();
        }
        let finished: Vec<_> = done
            .iter()
            .filter(|e| e.reason == NotifyReason::AgentFinished)
            .collect();
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].route, "#/review/85");
    }

    #[test]
    fn nobody_but_you_merges_after_followups_and_followups_count_for_the_team() {
        let mut w = ws();
        ok(&mut w, address(91, &["c1"], direct()), 1);
        let team = crate::team::members(&w);
        let agent = team.iter().find(|m| m.person.is_agent()).unwrap();
        assert_eq!(
            (agent.in_progress, agent.review),
            (1, 0),
            "BAU-138 zählt einmal, jetzt in Arbeit"
        );
        assert_eq!(agent.item_ids, ["bau138"]);
        run_to_end(&mut w);
        assert_eq!(
            w.apply(
                Action::MergePr {
                    review: 91,
                    method: MergeMethod::Squash
                },
                Actor::Agent,
                T
            ),
            Err(ActionError::MergeNotAllowed(Actor::Agent))
        );
    }

    #[test]
    fn followup_actions_and_runs_survive_json_and_old_saves() {
        let a = address(91, &["c1"], direct());
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
        for a in [
            Action::InvestigateCheck {
                review: 85,
                check: "Lint".into(),
            },
            Action::ApplyFix {
                run: "check-85-lint".into(),
            },
            Action::DiscardFix {
                run: "check-85-lint".into(),
            },
        ] {
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
        }
        // Mitten in der Diagnose speichern und laden.
        let mut w = ws();
        investigate_85(&mut w, "Lint");
        run_to_end(&mut w);
        let (back, _) = persist::restore(Some(&persist::save(&w)));
        assert_eq!(back, w);
        assert!(back.run("check-85-lint").unwrap().is_diagnosed());
        // Alte Läufe ohne `kind` sind „Ticket umsetzen“.
        let old: crate::agent::AgentRun =
            serde_json::from_str(r#"{"ticket_key":"BAU-1","state":"Done"}"#).unwrap();
        assert_eq!(old.kind, crate::agent::RunKind::Implement);
        assert_eq!(old.key(), "BAU-1");
        // Alte Kommentare ohne Autor gehören mir und sind offen.
        let c: crate::review::LineComment =
            serde_json::from_str(r#"{"id":"c1","file":"a","line":0,"text":"x"}"#).unwrap();
        assert!(c.is_own() && c.is_open());
    }
    use crate::agent::RunState;

    // ---------- Kosten, Grenzen, Regeln ----------

    fn advance(w: &mut Workspace, n: u64) {
        for i in 0..n {
            let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T + i);
        }
    }

    fn limits(f: impl FnOnce(&mut AgentLimits)) -> Action {
        let mut l = AgentLimits::default();
        f(&mut l);
        Action::SetAgentLimits { limits: l }
    }

    fn start(w: &mut Workspace, key: &str) {
        ok(
            w,
            Action::StartAgent {
                ticket: key.into(),
                options: direct(),
            },
            1,
        );
    }

    #[test]
    fn every_step_has_usage_and_lands_in_the_ledger() {
        let mut w = ws();
        start(&mut w, "BAU-150");
        advance(&mut w, STEPS as u64);
        let run = w.run("BAU-150").unwrap();
        assert!(run.is_done());
        assert_eq!(run.step_usage.len(), STEPS);
        let total = run.usage();
        assert!(total.input_tokens > 0 && total.output_tokens > 0);
        assert!(total.cost_cents > 0 && total.millis > 0);
        assert_eq!(w.spent_today(T), total);
        assert_eq!(w.spent_month(T), total);
        // Jeder Schritt zeigt seinen Verbrauch.
        assert!(run.steps().iter().all(|s| s.usage.is_some()));
        // Dieselben Eingaben, dieselben Werte.
        let mut w2 = ws();
        start(&mut w2, "BAU-150");
        advance(&mut w2, STEPS as u64);
        assert_eq!(w2.run("BAU-150").unwrap().usage(), total);
    }

    #[test]
    fn prices_are_a_setting_and_cost_follows_them() {
        let mut free = ws();
        ok(
            &mut free,
            limits(|l| {
                l.pricing.input_cents_per_mtok = 0;
                l.pricing.output_cents_per_mtok = 0;
            }),
            2,
        );
        start(&mut free, "BAU-150");
        advance(&mut free, STEPS as u64);
        let run = free.run("BAU-150").unwrap();
        assert_eq!(run.usage().cost_cents, 0);
        assert!(run.usage().tokens() > 0);
    }

    #[test]
    fn the_run_budget_stops_the_run_and_you_can_raise_it() {
        let mut w = ws();
        ok(&mut w, limits(|l| l.run_budget_cents = Some(20)), 1);
        start(&mut w, "BAU-150");
        advance(&mut w, 10);
        let run = w.run("BAU-150").unwrap();
        assert_eq!(run.state, RunState::Stopped(StopReason::RunBudget));
        assert!(run.completed < STEPS);
        assert!(run.usage().cost_cents >= 20);
        assert_eq!(w.stopped_agents(), 1);
        assert_eq!(w.waiting_agents(), 1);
        assert!(
            w.log
                .iter()
                .any(|e| e.text == "BAU-150: Angehalten – Budget pro Lauf erreicht")
        );
        // Ein angehaltener Lauf macht keinen Fortschritt.
        let done = run.completed;
        fails(&mut w, Action::AdvanceAgents, ActionError::NothingToDo);
        assert_eq!(w.run("BAU-150").unwrap().completed, done);
        // Ohne Erhöhung bleibt die Grenze erreicht, und nichts ändert sich.
        fails(
            &mut w,
            Action::ResumeStopped {
                run: "BAU-150".into(),
                extra: 0,
            },
            ActionError::LimitStillReached(StopReason::RunBudget),
        );
        let applied = ok(
            &mut w,
            Action::ResumeStopped {
                run: "BAU-150".into(),
                extra: 500,
            },
            1,
        );
        assert_eq!(
            applied.summary,
            "Agent für BAU-150 fortgesetzt – das Budget dieses Laufs um 5,00 € erhöht"
        );
        // Nur dieser Lauf bekommt mehr, die Einstellung bleibt.
        assert_eq!(w.limits.run_budget_cents, Some(20));
        advance(&mut w, 10);
        assert!(w.run("BAU-150").unwrap().is_done());
    }

    #[test]
    fn the_time_limit_stops_the_run() {
        let mut w = ws();
        ok(&mut w, limits(|l| l.run_minutes = Some(1)), 1);
        start(&mut w, "BAU-150");
        advance(&mut w, 10);
        let run = w.run("BAU-150").unwrap();
        assert_eq!(run.state, RunState::Stopped(StopReason::Time));
        ok(
            &mut w,
            Action::ResumeStopped {
                run: "BAU-150".into(),
                extra: 30,
            },
            1,
        );
        advance(&mut w, 10);
        assert!(w.run("BAU-150").unwrap().is_done());
    }

    #[test]
    fn the_month_budget_covers_all_runs_and_is_raised_in_the_settings() {
        let mut w = ws();
        ok(&mut w, limits(|l| l.month_budget_cents = Some(30)), 1);
        start(&mut w, "BAU-150");
        advance(&mut w, 10);
        assert_eq!(
            w.run("BAU-150").unwrap().state,
            RunState::Stopped(StopReason::MonthBudget)
        );
        // Ein zweiter Lauf hält sofort an: Das Monatsbudget ist schon aufgebraucht.
        start(&mut w, "BAU-142");
        advance(&mut w, 1);
        assert_eq!(
            w.run("BAU-142").unwrap().state,
            RunState::Stopped(StopReason::MonthBudget)
        );
        ok(
            &mut w,
            Action::ResumeStopped {
                run: "BAU-150".into(),
                extra: 10_000,
            },
            1,
        );
        assert_eq!(w.limits.month_budget_cents, Some(10_030));
        // Der andere Lauf geht danach ohne weitere Erhöhung weiter.
        ok(
            &mut w,
            Action::ResumeStopped {
                run: "BAU-142".into(),
                extra: 0,
            },
            1,
        );
    }

    #[test]
    fn a_stopped_run_can_be_cancelled_and_its_cost_stays_booked() {
        let mut w = ws();
        ok(&mut w, limits(|l| l.run_budget_cents = Some(20)), 1);
        start(&mut w, "BAU-150");
        advance(&mut w, 10);
        let spent = w.spent_month(T);
        assert!(spent.cost_cents >= 20);
        ok(
            &mut w,
            Action::CancelAgent {
                ticket: "BAU-150".into(),
            },
            1,
        );
        assert!(w.run("BAU-150").unwrap().is_idle());
        assert_eq!(w.spent_month(T), spent, "Kosten gehen nicht verloren");
        assert!(w.run("BAU-150").unwrap().step_usage.is_empty());
    }

    #[test]
    fn a_stopped_run_needs_attention_and_notifies() {
        let mut w = ws();
        ok(&mut w, limits(|l| l.run_budget_cents = Some(20)), 1);
        start(&mut w, "BAU-150");
        let mut before = w.clone();
        for i in 0..10 {
            if w.run("BAU-150").unwrap().is_stopped() {
                break;
            }
            before = w.clone();
            let _ = w.apply(Action::AdvanceAgents, Actor::Agent, T + i);
        }
        let events = crate::notify::events(&before, &w);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].reason, crate::NotifyReason::AgentStopped);
        assert_eq!(events[0].route, "#/agent/BAU-150");
        assert!(events[0].text.contains("Budget pro Lauf erreicht"));
        let stuck = crate::team::stuck(&w);
        let entry = stuck.iter().find(|s| s.key == "BAU-150").unwrap();
        assert_eq!(entry.kind, crate::StuckKind::AgentStopped);
        // Nur einmal melden.
        assert!(crate::notify::events(&w, &w).is_empty());
    }

    #[test]
    fn too_many_parallel_runs_do_not_start() {
        let mut w = ws();
        ok(&mut w, limits(|l| l.max_parallel = 1), 1);
        start(&mut w, "BAU-150");
        fails(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: direct(),
            },
            ActionError::ParallelLimit(1),
        );
        assert!(
            ActionError::ParallelLimit(1)
                .message()
                .starts_with("Es arbeiten schon 1 Agenten")
        );
        // Ein Lauf mit Plan wartet nur und belegt keinen Platz – erst die Freigabe zählt.
        ok(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: AgentOptions::default(),
            },
            2,
        );
        fails(
            &mut w,
            Action::ApprovePlan {
                ticket: "BAU-142".into(),
            },
            ActionError::ParallelLimit(1),
        );
        // Pausieren gibt den Platz frei; Fortsetzen braucht ihn wieder.
        ok(
            &mut w,
            Action::PauseAgent {
                run: "BAU-150".into(),
            },
            1,
        );
        ok(
            &mut w,
            Action::ApprovePlan {
                ticket: "BAU-142".into(),
            },
            1,
        );
        fails(
            &mut w,
            Action::ResumeAgent {
                run: "BAU-150".into(),
            },
            ActionError::ParallelLimit(1),
        );
    }

    #[test]
    fn limits_are_validated_and_logged() {
        let mut w = ws();
        fails(
            &mut w,
            limits(|l| l.max_parallel = 0),
            ActionError::InvalidInput,
        );
        fails(
            &mut w,
            limits(|l| l.run_budget_cents = Some(0)),
            ActionError::InvalidInput,
        );
        fails(&mut w, limits(|_| {}), ActionError::Unchanged);
        let applied = ok(&mut w, limits(|l| l.max_parallel = 2), 1);
        assert!(applied.summary.contains("höchstens 2 parallel"));
        assert_eq!(w.log.last().unwrap().target, Some(Target::Settings));
    }

    fn rules(f: impl FnOnce(&mut RepoRules)) -> Action {
        let mut r = RepoRules::default();
        f(&mut r);
        Action::SetRepoRules {
            repo: crate::rules::DEMO_REPO.into(),
            rules: r,
        }
    }

    #[test]
    fn a_plan_touching_forbidden_paths_cannot_be_approved() {
        let mut w = ws();
        ok(
            &mut w,
            rules(|r| r.forbidden_paths = vec!["tokens.css".into()]),
            1,
        );
        // Auch ohne gewünschten Plan wartet der Lauf auf die Freigabe.
        ok(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: direct(),
            },
            2,
        );
        let run = w.run("BAU-142").unwrap();
        assert!(run.is_awaiting());
        let plan = run.plan().unwrap();
        assert_eq!(plan.violations.len(), 1);
        assert_eq!(plan.violations[0].file, "tokens.css");
        assert_eq!(plan.rules, ["Verboten: tokens.css"]);
        let err = w
            .apply(
                Action::ApprovePlan {
                    ticket: "BAU-142".into(),
                },
                Actor::Du,
                T,
            )
            .unwrap_err();
        assert!(matches!(err, ActionError::PlanViolatesRules(_)));
        assert!(err.message().contains("tokens.css ist verboten"));
        assert!(w.run("BAU-142").unwrap().is_awaiting());
        // Auch das Zustandsmodell selbst gibt ihn nicht frei.
        assert!(!w.approve_plan("BAU-142"));
        // Nimmt man die Regel zurück, gilt das für den wartenden Plan sofort.
        ok(&mut w, rules(|_| {}), 1);
        ok(
            &mut w,
            Action::ApprovePlan {
                ticket: "BAU-142".into(),
            },
            1,
        );
        assert!(w.run("BAU-142").unwrap().is_running());
    }

    #[test]
    fn allowed_paths_limit_what_the_plan_may_touch() {
        let mut w = ws();
        ok(&mut w, rules(|r| r.allowed_paths = vec!["*.rs".into()]), 1);
        ok(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-142".into(),
                options: AgentOptions::default(),
            },
            2,
        );
        let plan = w.run("BAU-142").unwrap().plan().unwrap();
        assert_eq!(plan.violations.len(), 2);
        assert!(
            plan.violations[0]
                .reason()
                .contains("außerhalb der erlaubten")
        );
    }

    #[test]
    fn required_tests_and_plan_approval_are_enforced_at_the_start() {
        let mut w = ws();
        ok(
            &mut w,
            rules(|r| {
                r.required_tests = vec!["cargo test".into(), "cargo clippy".into()];
                r.plan_required = true;
                r.instructions = "Kleine Commits.".into();
            }),
            1,
        );
        let off = AgentOptions {
            run_tests: false,
            plan_first: false,
            ..AgentOptions::default()
        };
        ok(
            &mut w,
            Action::StartAgent {
                ticket: "BAU-150".into(),
                options: off,
            },
            2,
        );
        let run = w.run("BAU-150").unwrap();
        assert!(run.options.run_tests && run.options.plan_first);
        assert!(run.is_awaiting());
        let plan = run.plan().unwrap();
        assert!(plan.violations.is_empty());
        assert!(plan.rules.iter().any(|l| l.contains("Pflicht-Tests")));
        assert!(plan.rules.iter().any(|l| l == "Anweisung: Kleine Commits."));
        assert!(
            plan.steps
                .iter()
                .any(|s| s == "Pflicht-Tests ausführen: cargo test, cargo clippy")
        );
        ok(
            &mut w,
            Action::ApprovePlan {
                ticket: "BAU-150".into(),
            },
            1,
        );
        advance(&mut w, 10);
        let run = w.run("BAU-150").unwrap();
        let steps = run.steps();
        assert_eq!(steps[3].title, "Pflicht-Tests ausgeführt");
        assert!(run.log().contains("$ cargo clippy … bestanden"));
    }

    #[test]
    fn rules_are_per_repo_cleaned_and_validated() {
        let mut w = ws();
        fails(
            &mut w,
            Action::SetRepoRules {
                repo: "gibt/es-nicht".into(),
                rules: RepoRules::default(),
            },
            ActionError::UnknownRepo("gibt/es-nicht".into()),
        );
        ok(
            &mut w,
            rules(|r| r.forbidden_paths = vec!["  a/** ".into(), "".into()]),
            1,
        );
        assert_eq!(
            w.rules_for(crate::rules::DEMO_REPO).forbidden_paths,
            ["a/**"]
        );
        fails(
            &mut w,
            rules(|r| r.forbidden_paths = vec!["a/**".into()]),
            ActionError::Unchanged,
        );
        let applied = ok(&mut w, rules(|_| {}), 1);
        assert!(applied.summary.ends_with("entfernt"));
    }

    #[test]
    fn settings_survive_the_demo_reset_but_the_ledger_does_not() {
        let mut w = ws();
        ok(&mut w, limits(|l| l.max_parallel = 2), 1);
        start(&mut w, "BAU-150");
        advance(&mut w, 2);
        assert!(!w.spend.is_empty());
        w.apply(Action::ResetDemo, Actor::Du, T).unwrap();
        assert_eq!(w.limits.max_parallel, 2);
        assert!(w.spend.is_empty());
        assert!(!w.repo_rules.is_empty());
    }

    #[test]
    fn the_new_actions_roundtrip_and_old_saves_still_load() {
        for a in [
            Action::ResumeStopped {
                run: "BAU-1".into(),
                extra: 100,
            },
            limits(|l| l.max_parallel = 5),
            rules(|r| r.plan_required = true),
        ] {
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
        }
        // Ein Stand ohne die neuen Felder lädt mit Standardwerten.
        let mut w = ws();
        start(&mut w, "BAU-150");
        let mut v: serde_json::Value = serde_json::from_str(&persist::save(&w)).unwrap();
        let ws_obj = v["workspace"].as_object_mut().unwrap();
        for key in ["limits", "spend", "repo_rules"] {
            ws_obj.remove(key);
        }
        for run in ws_obj["runs"].as_array_mut().unwrap() {
            let run = run.as_object_mut().unwrap();
            for key in [
                "repo",
                "rules",
                "step_usage",
                "extra_budget_cents",
                "extra_minutes",
            ] {
                run.remove(key);
            }
        }
        let (back, restored) = persist::restore(Some(&v.to_string()));
        assert!(restored);
        assert_eq!(back.limits, AgentLimits::default());
        assert!(back.spend.is_empty());
        assert!(back.run("BAU-150").unwrap().is_running());
        // Der angehaltene Zustand übersteht das Speichern.
        let mut s = ws();
        ok(&mut s, limits(|l| l.run_budget_cents = Some(20)), 1);
        start(&mut s, "BAU-150");
        advance(&mut s, 10);
        let (back, _) = persist::restore(Some(&persist::save(&s)));
        assert_eq!(back, s);
        assert!(back.run("BAU-150").unwrap().is_stopped());
    }

    #[test]
    fn the_agent_cannot_merge_even_after_a_stop() {
        let mut w = ws();
        ok(&mut w, limits(|l| l.run_budget_cents = Some(20)), 1);
        start(&mut w, "BAU-150");
        advance(&mut w, 10);
        for actor in [Actor::Agent, Actor::System] {
            assert_eq!(
                w.apply(merge(87, MergeMethod::Squash), actor, T),
                Err(ActionError::MergeNotAllowed(actor))
            );
        }
    }
}
