//! Jira responses → types from `cockpit-core`. Pure functions, without network.

use cockpit_core::{
    Comment, Item, Kind, Person, ReasonKind, Scope, Source, Sprint, SprintState, TicketStatus,
};

use super::adf::{adf_to_text, adf_to_text_for};
use super::dto::{Issue, JiraComment};
use crate::{text, time};

/// Status from the status category of Jira (`new`/`indeterminate`/`done`). A status whose
/// name contains „Review“ counts as `Review`, since Jira has no category of its own for it.
fn status(issue: &Issue) -> TicketStatus {
    status_from(&issue.fields.status)
}

/// Like [`status`], for a single Jira status (also the target of a transition).
pub fn status_from(s: &super::dto::Status) -> TicketStatus {
    match s.status_category.as_ref().map(|c| c.key.as_str()) {
        Some("done") => TicketStatus::Done,
        Some("indeterminate") if s.name.to_lowercase().contains("review") => TicketStatus::Review,
        Some("indeterminate") => TicketStatus::InProgress,
        _ => TicketStatus::Open,
    }
}

/// A Jira issue as an inbox entry. `me` is the `accountId` of the user.
///
/// Assigned to me → `Scope::Mine`, reason `Assigned`. Only watched → `Scope::Team`, reason
/// `Comment` (provisional, the attention rules follow later). Without an assignee the owner is
/// „Nicht zugewiesen“ (the filter „Nicht zugewiesen“ relies on it). Comments are empty – they come through [`comments_to_core`].
pub fn issue_to_item(issue: &Issue, me: &str, now: i64) -> Item {
    issue_to_item_with(issue, me, now, None)
}

/// Like [`issue_to_item`]; `sprint_field` is the ID of the site's sprint field (see
/// `JiraClient::sprint_field`), from which the sprint of the ticket is read.
pub fn issue_to_item_with(issue: &Issue, me: &str, now: i64, sprint_field: Option<&str>) -> Item {
    let f = &issue.fields;
    let me_assigned = f.assignee.as_ref().is_some_and(|a| a.account_id == me);
    let owner = match f.assignee.as_ref() {
        Some(u) if u.account_id == me => Person::me(),
        Some(u) => text::person(&u.display_name),
        None => Person::new(cockpit_core::UNASSIGNED_NAME, "–"),
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
        description_rich: cockpit_core::adf::parse(&f.description, me),
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
        sprint: sprint_field.and_then(|id| sprint_of(f.extra.get(id)?)),
        updated_ms: time::parse_iso(&f.updated).map(|s| s as u64 * 1000),
    }
}

/// The sprint of a ticket from the value of the sprint field: a list of sprints (a ticket that
/// was carried over is in several). The active one counts, otherwise a future one, otherwise
/// the last closed one. Reads the objects of REST v3 and, for sites that still answer with
/// the old text form (`…Sprint@1f[id=7,state=ACTIVE,name=Sprint 7,…]`), that too.
pub fn sprint_of(value: &serde_json::Value) -> Option<Sprint> {
    let list: Vec<Sprint> = value
        .as_array()?
        .iter()
        .filter_map(|v| match v {
            serde_json::Value::Object(o) => {
                let text = |k: &str| o.get(k).and_then(|v| v.as_str()).unwrap_or_default();
                Some(Sprint {
                    name: text("name").to_string(),
                    state: SprintState::parse(text("state"))?,
                    start: text("startDate").to_string(),
                    end: text("endDate").to_string(),
                })
            }
            serde_json::Value::String(t) => sprint_from_text(t),
            _ => None,
        })
        .collect();
    [SprintState::Active, SprintState::Future]
        .into_iter()
        .find_map(|state| list.iter().find(|s| s.state == state).cloned())
        .or_else(|| list.last().cloned())
}

fn sprint_from_text(t: &str) -> Option<Sprint> {
    let inner = t.split_once('[')?.1.trim_end_matches(']');
    let part = |key: &str| {
        inner
            .split(',')
            .find_map(|kv| kv.trim().strip_prefix(key)?.strip_prefix('='))
            .unwrap_or_default()
            .to_string()
    };
    Some(Sprint {
        name: part("name"),
        state: SprintState::parse(&part("state"))?,
        start: part("startDate"),
        end: part("endDate"),
    })
}

/// Comments of an issue, in Jira's order (oldest first).
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
                rich: cockpit_core::adf::parse(&c.body, me),
                own,
            }
        })
        .collect()
}
