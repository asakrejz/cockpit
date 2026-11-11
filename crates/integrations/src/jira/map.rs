//! Jira-Antworten → Typen aus `cockpit-core`. Reine Funktionen, ohne Netzwerk.

use cockpit_core::{Comment, Item, Kind, Person, ReasonKind, Scope, Source, TicketStatus};

use super::adf::{adf_to_text, adf_to_text_fuer};
use super::dto::{Issue, JiraComment};
use crate::{text, zeit};

/// Status aus der Statuskategorie von Jira (`new`/`indeterminate`/`done`). Ein Status, dessen
/// Name „Review“ enthält, zählt als `Review`, da Jira dafür keine eigene Kategorie hat.
fn status(issue: &Issue) -> TicketStatus {
    status_von(&issue.fields.status)
}

/// Wie [`status`], für einen einzelnen Jira-Status (auch das Ziel einer Transition).
pub fn status_von(s: &super::dto::Status) -> TicketStatus {
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
pub fn issue_to_item(issue: &Issue, ich: &str, jetzt: i64) -> Item {
    let f = &issue.fields;
    let mir_zugewiesen = f.assignee.as_ref().is_some_and(|a| a.account_id == ich);
    let owner = match f.assignee.as_ref().or(f.reporter.as_ref()) {
        Some(u) if u.account_id == ich => Person::me(),
        Some(u) => text::person(&u.display_name),
        None => Person::new("Nicht zugewiesen", "–"),
    };
    let (reason, reason_text, scope) = if mir_zugewiesen {
        (ReasonKind::Assigned, "Dir zugewiesen", Scope::Mine)
    } else {
        (ReasonKind::Comment, "Beobachtet", Scope::Team)
    };
    Item {
        id: text::id_aus(&[&issue.key]),
        kind: Kind::Ticket,
        source: Source::Jira,
        key: issue.key.clone(),
        title: f.summary.clone(),
        description: adf_to_text(&f.description),
        reason,
        reason_text: reason_text.into(),
        time: zeit::relativ(&f.updated, jetzt),
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
    }
}

/// Kommentare eines Issues, in der Reihenfolge von Jira (älteste zuerst).
pub fn comments_to_core(kommentare: &[JiraComment], ich: &str, jetzt: i64) -> Vec<Comment> {
    kommentare
        .iter()
        .map(|c| {
            let eigener = c.author.account_id == ich;
            Comment {
                author: if eigener {
                    Person::me()
                } else {
                    text::person(&c.author.display_name)
                },
                time: zeit::relativ(&c.created, jetzt),
                text: adf_to_text_fuer(&c.body, ich),
                own: eigener,
            }
        })
        .collect()
}
