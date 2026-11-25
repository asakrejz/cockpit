//! Loading the items of GitHub Projects as tickets, for the sync.

use cockpit_core::{Loaded, Source, Workspace};
use cockpit_i18n::msg;

use super::ProjectsService;
use crate::Error;
use crate::github::projects::{self, Heavy, MapCtx};

/// An error that holds for every project: the source fails as a whole (and keeps its data).
fn all_projects(e: &Error) -> bool {
    matches!(
        e,
        Error::RateLimit { .. }
            | Error::NotSignedIn
            | Error::NoPermission
            | Error::MissingRight { .. }
    )
}

/// What the workspace knew about a project that could not be read this time stays.
fn carry_project(known: &Workspace, project: &cockpit_core::ProjectRef, out: &mut Loaded) {
    let name = project.to_string();
    for item in known
        .items
        .iter()
        .filter(|i| i.source == Source::GitHubProject && known.sync.is_synced(&i.id))
        .filter(|i| i.project.as_ref().is_some_and(|p| p.project == name))
    {
        if !out.items.iter().any(|i| i.id == item.id) {
            out.items.push(item.clone());
        }
    }
    if let Some(meta) = known
        .sync
        .projects
        .iter()
        .find(|m| m.reference() == *project)
    {
        out.projects.push(meta.clone());
    }
}

/// Loads the items of the chosen projects as tickets. Incremental: the list pass is light (fields,
/// assignees, labels), the heavy part (comments, linked pull requests, parent, sub-issues) is
/// fetched only for issues that are new or whose `updatedAt` moved; every other issue keeps what
/// the workspace already has. The GraphQL rate limit ends the sync early (the source keeps its data
/// and pauses until the limit resets).
pub async fn projects_load(
    svc: &ProjectsService,
    known: &Workspace,
    now_s: i64,
) -> Result<Loaded, Error> {
    let client = &svc.client;
    let me = client.me().await?;
    let team_field = svc
        .team_field
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(cockpit_core::project::DEFAULT_TEAM_FIELD);
    let ctx = MapCtx {
        me: &me,
        now_s,
        team_field,
    };
    let mut out = Loaded::default();
    let mut seen: Vec<String> = Vec::new();
    let mut extras_missing = false;
    for project in &svc.projects {
        let name = project.to_string();
        let meta = match client
            .project_meta(project, svc.status_field.as_deref())
            .await
        {
            Ok(m) => m,
            Err(e) if all_projects(&e) => return Err(e),
            Err(Error::NotFound) => {
                out.warn(msg!(IntegrationsSyncProjectNotFound, project = &name));
                carry_project(known, project, &mut out);
                continue;
            }
            Err(e) => {
                out.warn(msg!(
                    IntegrationsSyncProjectStale,
                    project = &name,
                    error = e.msg()
                ));
                carry_project(known, project, &mut out);
                continue;
            }
        };
        if meta.status_def().is_none() {
            out.warn(msg!(
                IntegrationsSyncProjectNoStatus,
                project = &name,
                field = svc
                    .status_field
                    .as_deref()
                    .unwrap_or(cockpit_core::project::DEFAULT_STATUS_FIELD)
            ));
        }
        let page = match client.project_items(&meta.id).await {
            Ok(p) => p,
            Err(e) if all_projects(&e) => return Err(e),
            Err(e) => {
                out.warn(msg!(
                    IntegrationsSyncProjectStale,
                    project = &name,
                    error = e.msg()
                ));
                carry_project(known, project, &mut out);
                continue;
            }
        };
        if page.truncated {
            out.warn(msg!(
                IntegrationsSyncProjectTruncated,
                project = &name,
                n = projects::MAX_ITEMS
            ));
        }

        // The light pass: map every item; decide which ones need the heavy part.
        let mut fresh: Vec<(String, cockpit_core::Item)> = Vec::new(); // (issue node ID, item)
        let mut ready: Vec<cockpit_core::Item> = Vec::new();
        for node in &page.items {
            let Some(mut item) = projects::map_item(node, &meta, &ctx) else {
                continue;
            };
            if seen.contains(&item.id) {
                continue;
            }
            seen.push(item.id.clone());
            let Some(content_id) = item
                .project
                .as_ref()
                .filter(|p| !p.draft)
                .map(|p| p.content_id.clone())
            else {
                // A draft has no comments or timeline.
                ready.push(item);
                continue;
            };
            let old = known
                .item(&item.id)
                .filter(|o| known.sync.is_synced(&o.id))
                .filter(|o| o.updated_ms.is_some() && o.updated_ms == item.updated_ms)
                .and_then(Heavy::of_item);
            match old {
                Some(heavy) => {
                    projects::apply_heavy(&mut item, heavy);
                    ready.push(item);
                }
                None => fresh.push((content_id, item)),
            }
        }
        // The heavy pass, only for what is new or changed.
        if !fresh.is_empty() {
            let ids: Vec<String> = fresh.iter().map(|(id, _)| id.clone()).collect();
            match client.item_details(&ids).await {
                Ok((details, extras)) => {
                    extras_missing |= !extras;
                    for (content_id, mut item) in fresh {
                        if let Some(d) = details.iter().find(|d| d.id == content_id) {
                            projects::apply_details(&mut item, d, &ctx);
                        }
                        ready.push(item);
                    }
                }
                Err(e) if all_projects(&e) => return Err(e),
                Err(e) => {
                    out.warn(msg!(
                        IntegrationsSyncProjectDetailsFailed,
                        project = &name,
                        error = e.msg()
                    ));
                    for (_, mut item) in fresh {
                        // Keep the old heavy part if there is one, and ask again next time.
                        if let Some(h) = known.item(&item.id).and_then(Heavy::of_item) {
                            projects::apply_heavy(&mut item, h);
                        }
                        item.updated_ms = None;
                        ready.push(item);
                    }
                }
            }
        }
        // The order of the project, not the order the heavy pass finished in.
        let order: Vec<&str> = page.items.iter().map(|n| n.id.as_str()).collect();
        ready.sort_by_key(|i| {
            order
                .iter()
                .position(|o| Some(*o) == i.project.as_ref().map(|p| p.item_id.as_str()))
                .unwrap_or(usize::MAX)
        });
        out.items.extend(ready);
        out.projects.push(meta);
    }
    if extras_missing {
        out.warn(msg!(IntegrationsSyncProjectNoExtras));
    }
    Ok(out)
}
