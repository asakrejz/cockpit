//! Writing to GitHub Projects that takes more than one call: a new issue in a project and the
//! conversion of a draft. Both only happen for an action the user clicked; the answer of GitHub
//! (number, IDs) is booked into the workspace by the same action.

use cockpit_core::api::ActionResponse;
use cockpit_core::{Action, Actor, CreatedIssue};
use cockpit_i18n::msg;

use crate::app::{AppState, now_ms};
use crate::live::Error as LiveError;
use crate::sync::ActionFailure;

impl AppState {
    pub(crate) async fn project_issue_flow(
        &self,
        action: Action,
    ) -> Result<ActionResponse, ActionFailure> {
        let inner = &self.inner;
        let _lock = inner.lock.lock().await;
        let before = inner.live.copy();
        let services = self.services().await;
        let Some(client) = services.projects.as_ref().map(|p| &p.client) else {
            return Err(ActionFailure::Plan(msg!(IntegrationsPlanProjectsOff)));
        };
        // Trial on a copy with a made-up answer: all the rules apply before GitHub is asked. What
        // the browser sent as `created` is never used.
        let placeholder = CreatedIssue {
            repo: "x/y".into(),
            number: u32::MAX,
            content_id: "I_trial".into(),
            item_id: "PVTI_trial".into(),
            url: String::new(),
        };
        let (trial_action, created) = match &action {
            Action::CreateIssue {
                project,
                repo,
                title,
                body,
                status,
                ..
            } => {
                let trial = Action::CreateIssue {
                    project: project.clone(),
                    repo: repo.clone(),
                    title: title.clone(),
                    body: body.clone(),
                    status: *status,
                    created: Some(CreatedIssue {
                        repo: repo.clone(),
                        ..placeholder
                    }),
                };
                let meta = before.sync.project(project);
                // The column the status stands for (the field and option IDs).
                let column = status.and_then(|s| {
                    let def = meta?.status_def()?;
                    let option = cockpit_core::project::option_for(&def.options, s)?;
                    Some((def.id.clone(), option.id.clone()))
                });
                (trial, Ok(column))
            }
            Action::ConvertDraft { item, repo, .. } => {
                let trial = Action::ConvertDraft {
                    item: item.clone(),
                    repo: repo.clone(),
                    created: Some(CreatedIssue {
                        repo: repo.clone(),
                        ..placeholder
                    }),
                };
                (trial, Err(()))
            }
            _ => return Err(ActionFailure::Intern),
        };
        let mut probe = before.clone();
        probe
            .apply(trial_action, Actor::User, now_ms())
            .map_err(ActionFailure::Rule)?;

        let service_failed = |e: cockpit_integrations::Error| {
            tracing::warn!("GitHub Projects: call failed: {e}");
            ActionFailure::Service(msg!(
                ServerRunServiceFailed,
                service = "GitHub".to_string(),
                error = e.msg()
            ))
        };
        let (done, written) = match action {
            Action::CreateIssue {
                project,
                repo,
                title,
                body,
                status,
                ..
            } => {
                let column = created.unwrap_or(None);
                let made = client
                    .create_issue_in_project(
                        &project,
                        &repo,
                        title.trim(),
                        body.trim(),
                        column.as_ref().map(|(f, o)| (f.as_str(), o.as_str())),
                    )
                    .await
                    .map_err(service_failed)?;
                (
                    Action::CreateIssue {
                        project,
                        repo,
                        title,
                        body,
                        status,
                        created: Some(made),
                    },
                    true,
                )
            }
            Action::ConvertDraft { item, repo, .. } => {
                let item_id = before
                    .item(&item)
                    .and_then(|i| i.project.as_ref())
                    .map(|p| p.item_id.clone())
                    .ok_or(ActionFailure::Intern)?;
                let made = client
                    .convert_draft(&item_id, &repo)
                    .await
                    .map_err(service_failed)?;
                (
                    Action::ConvertDraft {
                        item,
                        repo,
                        created: Some(made),
                    },
                    true,
                )
            }
            _ => return Err(ActionFailure::Intern),
        };
        if written {
            inner
                .written
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        match self.local_apply(done, Actor::User).await {
            Ok(answer) => Ok(answer),
            Err(LiveError::Action(e)) => Err(ActionFailure::Service(msg!(
                ServerRunServiceDoneNotApplied,
                error = e.msg()
            ))),
            Err(LiveError::Save(m)) => Err(ActionFailure::Save(m)),
        }
    }
}
