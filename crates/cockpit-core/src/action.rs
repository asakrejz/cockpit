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
use crate::attention::Rule;
use crate::demo;
use crate::model::TicketStatus;
use crate::review::{Decision, SendError, Verdict};
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
    AddLineComment {
        review: u32,
        file: String,
        line: usize,
        text: String,
    },
    EditLineComment {
        review: u32,
        comment: String,
        text: String,
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
            Action::Decide {
                review,
                suggestion,
                decision,
            } => {
                self.need_review(review)?;
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
                self.need_review(review)?;
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
            } => {
                self.need_review(review)?;
                if self.add_line_comment(review, &file, line, &text).is_none() {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!(
                        "Kommentar an {file}, Zeile {}, in #{review} hinzugefügt",
                        line + 1
                    ),
                )])
            }
            Action::EditLineComment {
                review,
                comment,
                text,
            } => {
                self.need_review(review)?;
                let known = self
                    .review(review)
                    .is_some_and(|r| r.line_comments.iter().any(|c| c.id == comment));
                if !known {
                    return Err(ActionError::UnknownComment(comment));
                }
                if !self.edit_line_comment(review, &comment, &text) {
                    return Err(ActionError::InvalidInput);
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Eigenen Kommentar in #{review} bearbeitet"),
                )])
            }
            Action::DeleteLineComment { review, comment } => {
                self.need_review(review)?;
                if !self.delete_line_comment(review, &comment) {
                    return Err(ActionError::UnknownComment(comment));
                }
                Ok(vec![Note::new(
                    Target::Review(review),
                    format!("Eigenen Kommentar in #{review} gelöscht"),
                )])
            }
            Action::SetVerdict { review, verdict } => {
                self.need_review(review)?;
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
                self.need_review(review)?;
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
    use crate::persist;
    use crate::review::Decision;

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
}
