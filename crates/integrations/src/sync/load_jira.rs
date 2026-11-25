//! Loading tickets from Jira, for the sync.

use cockpit_core::Loaded;
use cockpit_i18n::msg;

use crate::Error;
use crate::jira::{self, JiraClient};

/// Loads the tickets for `jql`. The default queries ask for the active sprints; if the site or
/// project has none (Jira rejects `openSprints()`, or there is no active sprint), the same query
/// with all open tickets runs instead and the state of the source says so. A JQL set by hand is
/// never rewritten and its errors stay errors.
pub async fn jira_load(
    client: &JiraClient,
    jql: &str,
    in_project: bool,
    prefs: &jira::FieldPrefs,
) -> Result<Loaded, Error> {
    let me = client.me().await?;
    let mut out = Loaded::default();
    // The sprint field is optional: without it (or without the right to list fields) the
    // tickets just carry no sprint.
    let sprint_field = match client.sprint_field().await {
        Ok(f) => f,
        Err(e @ (Error::RateLimit { .. } | Error::NotSignedIn)) => return Err(e),
        Err(e) => {
            out.warn(msg!(IntegrationsSyncSprintFieldFailed, error = e.msg()));
            None
        }
    };
    let field = sprint_field.as_deref();
    // The team field is optional too: without it no ticket has a team.
    let team_field = match client.team_field().await {
        Ok(f) => f,
        Err(e @ (Error::RateLimit { .. } | Error::NotSignedIn)) => return Err(e),
        Err(e) => {
            out.warn(msg!(IntegrationsSyncTeamFieldFailed, error = e.msg()));
            None
        }
    };
    let team = team_field.as_deref();
    let fallback = jira::open_tickets_jql(jql);
    let mut used_fallback = false;
    let page = match (
        client.search_page_fields(jql, &[field, team]).await,
        &fallback,
    ) {
        // No active sprint at all: show what is open instead.
        (Ok(list), Some(open)) if list.issues.is_empty() => {
            let all = client.search_page_fields(open, &[field, team]).await?;
            if !all.issues.is_empty() {
                used_fallback = true;
                let any_sprint = all.issues.iter().any(|i| {
                    field.is_some_and(|f| {
                        i.fields
                            .extra
                            .get(f)
                            .is_some_and(|v| jira::sprint_of(v).is_some())
                    })
                });
                out.warn(if any_sprint {
                    msg!(IntegrationsSyncNoActiveSprint)
                } else {
                    msg!(IntegrationsSyncNoSprints)
                });
            }
            all
        }
        (Ok(list), _) => list,
        // Jira does not know `openSprints()` here (e.g. a project without Jira Software).
        (Err(Error::Rejected(m)), Some(open))
            if m.is_empty() || m.to_lowercase().contains("sprint") =>
        {
            used_fallback = true;
            out.warn(msg!(IntegrationsSyncNoSprints));
            client.search_page_fields(open, &[field, team]).await?
        }
        (Err(e), _) => return Err(e),
    };
    if page.truncated {
        out.warn(msg!(IntegrationsSyncJiraTruncated, n = jira::MAX_ISSUES));
    }
    let issues = page.issues;
    // Whoever the ticket is not assigned to is shown because of the query, not because of a watch.
    let by_query = if in_project || used_fallback {
        Some(msg!(ReasonInProject))
    } else if jira::is_sprint_query(jql) {
        Some(msg!(ReasonInSprint))
    } else {
        None
    };
    for issue in issues {
        let mut item = jira::issue_to_item_full(&issue, &me, field, team);
        if let Some(text) = &by_query
            && item.scope != cockpit_core::Scope::Mine
        {
            item.reason_msg = Some(text.clone());
        }
        match client.comments(&issue.key).await {
            Ok(k) => item.comments = jira::comments_to_core(&k, &me),
            Err(Error::NotFound) => continue,
            Err(e) => return Err(e),
        }
        // Fields and attachments come with the search; images in the texts find their file.
        let details = jira::details_of(&issue, &me, field, prefs);
        for doc in item
            .description_rich
            .iter_mut()
            .chain(item.comments.iter_mut().filter_map(|c| c.rich.as_mut()))
        {
            cockpit_core::details::link_media(doc, &details.attachments);
        }
        item.url = client.browse_url(&issue.key);
        item.details = Some(details);
        out.items.push(item);
    }
    Ok(out)
}
