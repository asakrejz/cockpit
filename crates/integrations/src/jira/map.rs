//! Jira-Antworten → Typen aus `cockpit-core`. Reine Funktionen, ohne Netzwerk.

use cockpit_core::{Comment, Item, Kind, Person, ReasonKind, Scope, Source, TicketStatus};

use super::adf::{adf_to_text, adf_to_text_for};
use super::dto::{Issue, JiraComment};
use crate::{text, time};

/// Status aus der Statuskategorie von Jira (`new`/`indeterminate`/`done`). Ein Status, dessen
/// Name „Review“ enthält, zählt als `Review`, da Jira dafür keine eigene Kategorie hat.
fn status(issue: &Issue) -> TicketStatus {
    status_from(&issue.fields.status)
}

/// Wie [`status`], für einen einzelnen Jira-Status (auch das Ziel einer Transition).
pub fn status_from(s: &super::dto::Status) -> TicketStatus {
    match s.status_category.as_ref().map(|c| c.key.as_str()) {
        Some("done") => TicketStatus::Done,
        Some("indeterminate") if s.name.to_lowercase().contains("review") => TicketStatus::Review,
        Some("indeterminate") => TicketStatus::InProgress,
        _ => TicketStatus::Open,
    }
}

/// Ein Jira-Issue als Eintrag der Inbox. `ich` ist die `accountId` des Nutzers.
///
/// Mir zugewiesen → `Scope::Mine`, Grund `Assigned`. Nur beobachtet → `Scope::Team`, Grund
/// `Comment` (vorläufig, die Aufmerksamkeits-Regeln folgen später). Ohne Bearbeiter ist der
/// Melder der Besitzer. Kommentare sind leer – sie kommen über [`comments_to_core`].
pub fn issue_to_item(issue: &Issue, me: &str, now: i64) -> Item {
    let f = &issue.fields;
    let me_assigned = f.assignee.as_ref().is_some_and(|a| a.account_id == me);
    let owner = match f.assignee.as_ref().or(f.reporter.as_ref()) {
        Some(u) if u.account_id == me => Person::me(),
        Some(u) => text::person(&u.display_name),
        None => Person::new("Nicht zugewiesen", "–"),
    };
    let (reason, reason_text, scope) = if me_assigned {
        (ReasonKind::Assigned, "Dir zugewiesen", Scope::Mine)
    } else {
        (ReasonKind::Comment, "Beobachtet", Scope::Team)
    };
    Item {
        id: text::id_from(&[&issue.key]),
        kind: Kind::Ticket,
        source: Source::Jira,
        key: issue.key.clone(),
        title: f.summary.clone(),
        description: adf_to_text(&f.description),
        reason,
        reason_text: reason_text.into(),
        time: time::relative(&f.updated, now),
        scope,
        owner,
        status: status(issue),
        priority: cockpit_core::Priority::default(),
        branch: None,
        pr: None,
        agent_note: None,
        comments: Vec::new(),
        read: false,
        due: f.duedate.clone().filter(|d| !d.is_empty()),
        repo: None,
        labels: f.labels.iter().filter(|l| !l.is_empty()).cloned().collect(),
    }
}

/// Kommentare eines Issues, in der Reihenfolge von Jira (älteste zuerst).
pub fn comments_to_core(comments: &[JiraComment], me: &str, now: i64) -> Vec<Comment> {
    comments
        .iter()
        .map(|c| {
            let own = c.author.account_id == me;
            Comment {
                author: if own {
                    Person::me()
                } else {
                    text::person(&c.author.display_name)
                },
                time: time::relative(&c.created, now),
                text: adf_to_text_for(&c.body, me),
                own: own,
            }
        })
        .collect()
}
