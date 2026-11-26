//! The queries (JQL) the sync runs. Pure functions; nothing here talks to Jira.

use cockpit_i18n::msg;

use crate::Error;

/// Issues assigned to me or watched by me that are not done yet.
pub const ASSIGNED_OR_WATCHED_JQL: &str = "(assignee = currentUser() OR watcher = currentUser()) \
     AND statusCategory != Done ORDER BY updated DESC";

/// The part of the default queries that restricts to the active sprints.
pub const SPRINT_CLAUSE: &str = "sprint in openSprints()";
/// What the fallback puts in its place: all open tickets.
const OPEN_CLAUSE: &str = "statusCategory != Done";
const ORDER: &str = "ORDER BY updated DESC";

/// The query for the sync: a custom JQL stays as it is; otherwise the tickets of the active
/// sprints – restricted to `projects` if given (all projects the key sees otherwise). Project
/// keys are checked (letters, digits, `_`), so that nothing slips into the query.
pub fn jql_for(projects: &[String], own: Option<&str>) -> Result<String, Error> {
    if let Some(j) = own.map(str::trim).filter(|j| !j.is_empty()) {
        return Ok(j.to_string());
    }
    if projects.is_empty() {
        return Ok(format!("{SPRINT_CLAUSE} {ORDER}"));
    }
    for p in projects {
        if !cockpit_core::is_project_key(p) {
            return Err(Error::Coded(msg!(
                IntegrationsJiraBadProjectKey,
                project = p
            )));
        }
    }
    Ok(format!(
        "{SPRINT_CLAUSE} AND project in ({}) {ORDER}",
        projects.join(", ")
    ))
}

/// The tickets of the active sprint of a project.
pub fn jql_for_project(project: &str) -> Result<String, Error> {
    if !cockpit_core::is_project_key(project) {
        return Err(Error::Coded(msg!(
            IntegrationsJiraBadProjectKey,
            project = project
        )));
    }
    Ok(format!("project = {project} AND {SPRINT_CLAUSE} {ORDER}"))
}

/// The query the sync runs and how its tickets are labelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JiraQuery {
    pub jql: String,
    /// The query was chosen by the user: tickets that are neither
    /// assigned to me nor watched by me then show up with the reason `reason.in_project`.
    pub in_selection: bool,
}

/// The query for the sync, from the user's choice.
///
/// * Nothing chosen (`chosen` is `None` and the sprint view is "Current sprint"):
///   [`jql_for`] with the settings file.
/// * Project chosen, current sprint: [`jql_for_project`].
/// * Another sprint view ("All open", "Recently updated", "Backlog"): all open tickets
///   of the project (or of the projects of the settings file; all projects if there are none) –
///   whoever they are assigned to. A JQL set by hand is only replaced when a project is chosen.
pub fn jql_for_selection(
    projects: &[String],
    own: Option<&str>,
    chosen: Option<&str>,
    sprint: cockpit_core::SprintView,
) -> Result<JiraQuery, Error> {
    use cockpit_core::SprintView;
    let plain = |jql: String, in_selection| JiraQuery { jql, in_selection };
    if sprint == SprintView::Current {
        return match chosen {
            Some(p) => jql_for_project(p).map(|j| plain(j, true)),
            None => jql_for(projects, own).map(|j| plain(j, false)),
        };
    }
    if chosen.is_none() && own.map(str::trim).is_some_and(|j| !j.is_empty()) {
        return jql_for(projects, own).map(|j| plain(j, false));
    }
    let keys = chosen.map(|p| vec![p.to_string()]);
    let keys = keys.as_deref().unwrap_or(projects);
    for p in keys {
        if !cockpit_core::is_project_key(p) {
            return Err(Error::Coded(msg!(
                IntegrationsJiraBadProjectKey,
                project = p
            )));
        }
    }
    let mut clauses = Vec::new();
    if !keys.is_empty() {
        clauses.push(format!("project in ({})", keys.join(", ")));
    }
    clauses.push(OPEN_CLAUSE.to_string());
    match sprint {
        SprintView::Backlog => clauses.push("sprint is EMPTY".to_string()),
        SprintView::Recent => {
            clauses.push(format!("updated >= -{}d", cockpit_core::RECENT_DAYS));
        }
        SprintView::AllOpen | SprintView::Current => {}
    }
    Ok(plain(format!("{} {ORDER}", clauses.join(" AND ")), true))
}

/// Is this one of the two queries [`jql_for`] and [`jql_for_project`] build by default?
pub fn is_sprint_query(jql: &str) -> bool {
    let jql = jql.trim();
    jql.ends_with(ORDER)
        && (jql.starts_with(&format!("{SPRINT_CLAUSE} "))
            || jql.contains(&format!(" AND {SPRINT_CLAUSE} {ORDER}")))
}

/// The query for projects without sprints: the same default query with all open tickets
/// instead of the active sprints. `None` for any other query – a JQL set by hand is never
/// rewritten.
pub fn open_tickets_jql(jql: &str) -> Option<String> {
    is_sprint_query(jql).then(|| jql.trim().replacen(SPRINT_CLAUSE, OPEN_CLAUSE, 1))
}
