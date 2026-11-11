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

use crate::agent::{AgentOptions, STEPS};
use crate::demo;
use crate::model::{Checks, Item, TicketStatus};
use crate::review::{Decision, MergeBlocker, MergeMethod, SendError, Verdict};
use crate::workspace::{AgentError, Workspace};

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
    /// Der Takt: jeder laufende Agent schließt einen Schritt ab.
    AdvanceAgents,
    // Sonstiges
    /// Zurück zu den Beispieldaten. Das Protokoll beginnt neu.
    ResetDemo,
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
    /// Gemergt wird nur von Hand: weder der Agent noch das System dürfen es.
    MergeNotAllowed(Actor),
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
                "Der Agent arbeitet schon an diesem Ticket".into()
            }
            ActionError::UnknownCheck(name) => format!("Check „{name}“ gibt es nicht"),
            ActionError::Merged(n) => format!("#{n} ist schon gemergt und bleibt, wie es ist"),
            ActionError::MergeNotAllowed(actor) => format!(
                "Nur du darfst mergen – {} nicht",
                match actor {
                    Actor::Agent => "der Agent",
                    Actor::System => "das System",
                    Actor::Du => "du",
                }
            ),
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
    target: Target,
    text: String,
}

impl Note {
    fn new(target: Target, text: impl Into<String>) -> Self {
        Self {
            actor: None,
            target,
            text: text.into(),
        }
    }

    fn by(mut self, actor: Actor) -> Self {
        self.actor = Some(actor);
        self
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
        let notes = self.perform(action)?;
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

    fn push_log(&mut self, at_ms: u64, actor: Actor, target: Target, text: String) {
        let id = self.log.last().map_or(1, |e| e.id + 1);
        self.log.push(LogEntry {
            id,
            at_ms,
            actor,
            target: Some(target),
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
    fn perform(&mut self, action: Action) -> Result<Vec<Note>, ActionError> {
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
                self.merge_pr(review, method)
                    .map_err(ActionError::MergeBlocked)?;
                let ticket = self
                    .review(review)
                    .and_then(|r| r.ticket_key.clone())
                    .filter(|key| self.item_by_key(key).is_some_and(Item::is_ticket));
                let mut text = format!("#{review} gemergt ({})", method.label());
                if let Some(key) = ticket {
                    text.push_str(&format!(" – {key} ist fertig"));
                }
                Ok(vec![Note::new(Target::Review(review), text)])
            }
            Action::StartAgent { ticket, options } => {
                let plan_first = options.plan_first;
                self.start_agent(&ticket, options)
                    .map_err(ActionError::Agent)?;
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
                if !self
                    .runs
                    .iter()
                    .any(|r| r.ticket_key == ticket && r.is_active())
                {
                    return Err(ActionError::NothingToDo);
                }
                self.cancel_agent(&ticket);
                Ok(vec![Note::new(
                    Target::Run(ticket.clone()),
                    format!("Lauf des Agenten für {ticket} abgebrochen"),
                )])
            }
            Action::ApprovePlan { ticket } => {
                if !self.approve_plan(&ticket) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(ticket.clone()),
                    format!("Plan für {ticket} freigegeben"),
                )])
            }
            Action::RejectPlan { ticket } => {
                if !self.reject_plan(&ticket) {
                    return Err(ActionError::NothingToDo);
                }
                Ok(vec![Note::new(
                    Target::Run(ticket.clone()),
                    format!("Plan für {ticket} abgelehnt"),
                )])
            }
            Action::RevisePlan { ticket, note } => {
                let waiting = self
                    .runs
                    .iter()
                    .any(|r| r.ticket_key == ticket && r.is_awaiting());
                if !waiting {
                    return Err(ActionError::NothingToDo);
                }
                if !self.revise_plan(&ticket, &note) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Run(ticket.clone()),
                    format!("Neuen Plan für {ticket} angefordert"),
                )])
            }
            Action::AdvanceAgents => self.advance_logged(),
            Action::ResetDemo => {
                *self = demo::workspace();
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

    /// Der Takt der Agenten, mit einem Eintrag pro abgeschlossenem Schritt.
    fn advance_logged(&mut self) -> Result<Vec<Note>, ActionError> {
        // Vorher festhalten, welcher Schritt gleich fertig wird.
        let stepping: Vec<(String, usize, String, Option<u32>, bool)> = self
            .runs
            .iter()
            .filter(|r| r.is_running())
            .map(|r| {
                let title = r.steps()[r.completed.min(STEPS - 1)].title.clone();
                (
                    r.ticket_key.clone(),
                    r.completed,
                    title,
                    r.pr_number,
                    r.options.draft_pr,
                )
            })
            .collect();
        if stepping.is_empty() {
            return Err(ActionError::NothingToDo);
        }
        self.advance_agents();
        Ok(stepping
            .into_iter()
            .map(
                |(key, done, title, pr, draft)| match (done + 1 == STEPS, pr) {
                    (true, Some(n)) => Note::new(
                        Target::Review(n),
                        format!(
                            "{key}: Pull Request #{n} {}erstellt",
                            if draft { "als Entwurf " } else { "" }
                        ),
                    ),
                    _ => Note::new(Target::Run(key.clone()), format!("{key}: {title}")),
                },
            )
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ReasonKind, Tone};
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
        for i in 0..STEPS {
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
                "BAU-142: Änderungen umgesetzt",
                "BAU-142: Tests ausgeführt",
                "BAU-142: Pull Request #92 als Entwurf erstellt",
            ]
        );
        let last = w.log.last().unwrap();
        assert_eq!(last.target, Some(Target::Review(92)));
        assert_eq!(w.log[1].target, Some(Target::Run("BAU-142".into())));
        assert_eq!(w.log_entries(Some(Actor::Du)).len(), 1);
        assert_eq!(w.log_entries(None).len(), 6);
        assert_eq!(w.log_entries(Some(Actor::System)).len(), 0);
        assert_eq!(w.log_entries(None)[0].id, 6, "neueste zuerst");
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
    fn actor_ids_roundtrip() {
        for a in Actor::ALL {
            assert_eq!(Actor::from_id(a.id()), Some(a));
        }
        assert_eq!(Actor::from_id("x"), None);
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
        let applied = ok(&mut w, merge(91, MergeMethod::Rebase), 1);
        assert_eq!(applied.summary, "#91 gemergt (Rebase) – BAU-138 ist fertig");
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
}
