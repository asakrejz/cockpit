//! From an action to the call: [`plan_call`] decides which [`Outbound`] an [`Action`] becomes.
//! Pure, no network.

use cockpit_core::{Action, PrKey, Workspace};

use super::{Outbound, PlanError, Targets, review_data};

/// Determines the call for an action. `before` is the state before, `after` the state after
/// a trial application (that is how we know e.g. the name of the new branch). `Ok(None)`:
/// stays local.
pub fn plan_call(
    before: &Workspace,
    after: &Workspace,
    action: &Action,
    targets: &Targets,
) -> Result<Option<Outbound>, PlanError> {
    // `(repo, head commit, number)` of a synced pull request.
    let repo_of_pr = |key: &PrKey| -> Option<(String, String, u32)> {
        before
            .sync
            .pr(key)
            .filter(|_| targets.github)
            .map(|l| (l.repo.clone(), l.head_sha.clone(), l.number))
    };
    if let Some(planned) = plan_project(before, action, targets) {
        return planned;
    }
    Ok(match action {
        Action::Reply { item, text } => {
            let Some(i) = before.item(item).filter(|i| before.sync.is_synced(&i.id)) else {
                return Ok(None);
            };
            let text = text.trim().to_string();
            if text.is_empty() {
                return Ok(None);
            }
            match (i.source, i.pr.as_ref()) {
                (cockpit_core::Source::Jira, _) if targets.jira => Some(Outbound::JiraComment {
                    key: i.key.clone(),
                    text,
                }),
                (cockpit_core::Source::GitHub, Some(pr)) => repo_of_pr(&pr.key())
                    .map(|(repo, _, number)| Outbound::GitHubComment { repo, number, text }),
                _ => None,
            }
        }
        Action::SetStatus { item, status } => {
            let Some(i) = before.item(item).filter(|i| before.sync.is_synced(&i.id)) else {
                return Ok(None);
            };
            (i.source == cockpit_core::Source::Jira && i.is_ticket() && targets.jira).then(|| {
                Outbound::JiraStatus {
                    key: i.key.clone(),
                    status: *status,
                }
            })
        }
        Action::CreateBranch { item } => {
            let Some(i) = before.item(item).filter(|i| before.sync.is_synced(&i.id)) else {
                return Ok(None);
            };
            if !(i.source == cockpit_core::Source::Jira && targets.github) {
                return Ok(None);
            }
            // The default repo; without one the user has to set it (the button offers no choice).
            let Some(repo) = targets.default_repo.as_ref() else {
                return Err(PlanError(cockpit_i18n::msg!(IntegrationsPlanChooseRepo)));
            };
            let name = after
                .item(item)
                .and_then(|n| n.branch.clone())
                .ok_or_else(|| PlanError(cockpit_i18n::msg!(IntegrationsPlanNoBranchName)))?;
            Some(Outbound::GitHubBranch {
                repo: repo.clone(),
                name,
            })
        }
        Action::SendReview { review, comment } => {
            let (Some(r), Some((repo, sha, number))) = (before.review(review), repo_of_pr(review))
            else {
                return Ok(None);
            };
            Some(Outbound::GitHubReview {
                repo,
                number,
                review: review_data(r, comment, &sha),
            })
        }
        Action::MergePr { review, method } => {
            repo_of_pr(review).map(|(repo, sha, number)| Outbound::GitHubMerge {
                repo,
                number,
                method: *method,
                sha,
            })
        }
        Action::RestartCheck { review, check } => {
            repo_of_pr(review).map(|(repo, _, number)| Outbound::GitHubRecheck {
                repo,
                number,
                check: check.clone(),
            })
        }
        _ => None,
    })
}

// ---------- GitHub Projects ----------

/// The call for an action on a ticket of a GitHub project. `None` = the action does not concern one
/// (the caller goes on with Jira and pull requests). A ticket from a project needs the connection
/// to GitHub Projects: without it nothing is changed locally either, because the next sync would
/// undo it.
fn plan_project(
    before: &Workspace,
    action: &Action,
    targets: &Targets,
) -> Option<Result<Option<Outbound>, PlanError>> {
    use crate::github::projects::{Assignee, IssueWrite, priority_of};
    use cockpit_core::project::option_for;
    use cockpit_core::{FieldInput, Person};
    use cockpit_i18n::msg;

    let id = match action {
        Action::Reply { item, .. }
        | Action::SetStatus { item, .. }
        | Action::EditTicket { item, .. }
        | Action::AssignTicket { item, .. }
        | Action::SetPriority { item, .. }
        | Action::SetLabel { item, .. }
        | Action::SetIssueClosed { item, .. }
        | Action::SetProjectField { item, .. }
        | Action::ConvertDraft { item, .. } => item,
        _ => return None,
    };
    let item = before
        .item(id)
        .filter(|i| i.source == cockpit_core::Source::GitHubProject)
        .filter(|i| before.sync.is_synced(&i.id))?;
    let p = item.project.as_ref()?;
    // Creating and converting have flows of their own (several calls, the answer is booked).
    if matches!(action, Action::ConvertDraft { .. }) {
        return Some(Ok(None));
    }
    let fail = |m: cockpit_i18n::Msg| Some(Err(PlanError(m)));
    if !targets.projects {
        return fail(msg!(IntegrationsPlanProjectsOff));
    }
    let issue = || -> Option<(String, u32)> { Some((item.repo.clone()?, p.number?)) };
    let field_call = |field_id: &str, value: Option<FieldInput>| Outbound::ProjectField {
        project_id: p.project_id.clone(),
        item_id: p.item_id.clone(),
        field_id: field_id.to_string(),
        value,
    };
    let issue_call = |write: IssueWrite| {
        issue().map(|(repo, number)| Outbound::Issue {
            repo,
            number,
            write,
        })
    };
    Some(Ok(match action {
        Action::Reply { text, .. } => {
            let text = text.trim().to_string();
            if text.is_empty() {
                return Some(Ok(None));
            }
            if p.draft {
                return fail(msg!(IntegrationsPlanDraftNoComments));
            }
            issue_call(IssueWrite::Comment(text))
        }
        Action::SetStatus { status, .. } => {
            let Some(meta) = before.sync.project(&p.project_id) else {
                return fail(msg!(IntegrationsPlanNoProject));
            };
            let Some(def) = meta.status_def() else {
                return fail(msg!(IntegrationsPlanNoStatusField));
            };
            let Some(option) = option_for(&def.options, *status) else {
                return fail(msg!(IntegrationsPlanNoColumn, status = status.name_msg()));
            };
            Some(field_call(
                &def.id,
                Some(FieldInput::Select(option.id.clone())),
            ))
        }
        Action::EditTicket {
            title, description, ..
        } => {
            let (title, body) = (title.trim().to_string(), description.trim().to_string());
            if p.draft {
                Some(Outbound::DraftEdit {
                    draft_id: p.content_id.clone(),
                    title,
                    body,
                })
            } else {
                issue_call(IssueWrite::Edit { title, body })
            }
        }
        Action::AssignTicket { person, .. } => {
            if p.draft {
                return fail(msg!(IntegrationsPlanDraftNoAssignee));
            }
            let who = if *person == Person::me() {
                Assignee::Me
            } else if person.nobody {
                Assignee::Nobody
            } else {
                Assignee::Login(person.name.clone())
            };
            issue_call(IssueWrite::Assign(who))
        }
        Action::SetPriority { priority, .. } => {
            let Some(meta) = before.sync.project(&p.project_id) else {
                return fail(msg!(IntegrationsPlanNoProject));
            };
            let def = meta
                .field_named("Priority")
                .filter(|d| d.kind == cockpit_core::FieldKind::SingleSelect);
            let option = def.and_then(|d| {
                d.options
                    .iter()
                    .find(|o| priority_of(&o.name) == *priority)
                    .map(|o| (d.id.clone(), o.id.clone()))
            });
            let Some((field, option)) = option else {
                return fail(msg!(IntegrationsPlanNoPriority));
            };
            Some(field_call(&field, Some(FieldInput::Select(option))))
        }
        Action::SetLabel { label, on, .. } => issue_call(IssueWrite::Label {
            label: label.trim().to_string(),
            on: *on,
        }),
        Action::SetIssueClosed { closed, .. } => issue_call(IssueWrite::State { closed: *closed }),
        Action::SetProjectField {
            field_id, value, ..
        } => Some(field_call(field_id, value.clone())),
        _ => return None,
    }))
}
