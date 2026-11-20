//! The Status field of a project as the ticket status, and back.

use super::SelectOption;
use crate::model::TicketStatus;

/// The default columns Cockpit creates in a new project, in order.
pub const DEFAULT_COLUMNS: [(&str, TicketStatus); 4] = [
    ("Todo", TicketStatus::Open),
    ("In Progress", TicketStatus::InProgress),
    ("In Review", TicketStatus::Review),
    ("Done", TicketStatus::Done),
];

/// The status a column name stands for, by its words. `None` = no idea.
///
/// ```
/// use cockpit_core::project::classify;
/// use cockpit_core::TicketStatus;
/// assert_eq!(classify("In Progress"), Some(TicketStatus::InProgress));
/// assert_eq!(classify("In review"), Some(TicketStatus::Review));
/// assert_eq!(classify("Backlog"), Some(TicketStatus::Open));
/// assert_eq!(classify("Parked"), None);
/// ```
pub fn classify(name: &str) -> Option<TicketStatus> {
    let n = name.trim().to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    // Order matters: "In Review" holds "in", "Done" must not be hidden by "progress" words.
    if has(&["review", "testing", "qa", "verify", "prüf"]) {
        Some(TicketStatus::Review)
    } else if has(&[
        "done", "closed", "complete", "finished", "shipped", "released", "merged", "fertig",
        "erledigt", "abgeschlossen",
    ]) {
        // "shipped" is a done word here, listed so that a project with it still maps.
        Some(TicketStatus::Done)
    } else if has(&[
        "progress", "doing", "active", "started", "working", "development", "wip", "arbeit",
    ]) {
        Some(TicketStatus::InProgress)
    } else if has(&[
        "todo", "to do", "to-do", "backlog", "open", "new", "ready", "planned", "triage", "offen",
    ]) {
        Some(TicketStatus::Open)
    } else {
        None
    }
}

/// Which ticket status each option of the status field stands for. Names that are not
/// recognized take the place in the row: first → open, last → done, between → in progress.
pub fn map_options(options: &[SelectOption]) -> Vec<(String, TicketStatus)> {
    let last = options.len().saturating_sub(1);
    options
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let status = classify(&o.name).unwrap_or(if i == 0 {
                TicketStatus::Open
            } else if i == last {
                TicketStatus::Done
            } else {
                TicketStatus::InProgress
            });
            (o.id.clone(), status)
        })
        .collect()
}

/// The ticket status of the option `option_id`; no option → open.
pub fn status_of(options: &[SelectOption], option_id: Option<&str>) -> TicketStatus {
    option_id
        .and_then(|id| {
            map_options(options)
                .into_iter()
                .find(|(o, _)| o == id)
                .map(|(_, s)| s)
        })
        .unwrap_or_default()
}

/// The option that stands for `status` (the first one if several do). `None` = the field has no
/// such column, e.g. no review column.
pub fn option_for<'a>(options: &'a [SelectOption], status: TicketStatus) -> Option<&'a SelectOption> {
    map_options(options)
        .into_iter()
        .find(|(_, s)| *s == status)
        .and_then(|(id, _)| options.iter().find(|o| o.id == id))
}

/// The columns a status field still lacks to hold the four default columns (by meaning, not
/// name): used when Cockpit creates or completes a project.
pub fn missing_columns(options: &[SelectOption]) -> Vec<&'static str> {
    let mapped = map_options(options);
    DEFAULT_COLUMNS
        .iter()
        .filter(|(_, s)| !mapped.iter().any(|(_, m)| m == s))
        .map(|(n, _)| *n)
        .collect()
}
