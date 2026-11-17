//! Follow-up jobs and the pull request of the real agent, the part that talks to GitHub.
//!
//! * **Inputs of follow-up jobs** (review threads, failed checks) are only *read* from GitHub with the
//!   token of the person – at click time (to refuse an empty job early) and again when the run starts.
//!   They go into the job file as data. The container never gets the token.
//! * **The pull request** is only created on an explicit action of the person
//!   (`Action::CreatePullRequest`, after a preview with editable title and text). An existing open
//!   pull request for the branch is found and linked instead of creating a second one.
//!   Nothing here merges, changes the draft state or requests reviews.
//!
//! Errors come back as German sentences that say what to do; a token never appears in them.

use cockpit_core::api::PullRequestPreview;
use cockpit_core::{Action, Actor, FollowupTask, PullRequestRef, RunKind, Workspace};
use cockpit_integrations::Error as ServiceError;
use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::github::dto::PullSummary;

use crate::agent_run::FollowupJob;
use crate::app::{AppState, now_ms};
use crate::config::DEFAULT_GITHUB_API;
use crate::live::Error as LiveError;
use crate::sync::ActionFailure;

/// A sentence for the person: what failed at GitHub and what to do about it.
pub(crate) fn github_message(what: &str, e: &ServiceError) -> String {
    match e {
        ServiceError::NotSignedIn => format!(
            "{what}: GitHub hat den Zugang abgelehnt (401). Melde dich unter „Einrichten“ › „GitHub“ neu an oder trage einen gültigen Token ein."
        ),
        ServiceError::NoPermission => format!(
            "{what}: GitHub erlaubt das nicht (403). Der Zugang braucht für dieses Repo die Berechtigung „Pull requests“ (lesen und schreiben) und „Checks“ (lesen)."
        ),
        ServiceError::NotFound => format!(
            "{what}: GitHub kennt das Repo, den Branch oder den Pull Request nicht (404). Prüfe das Repo unter „Einrichten“ › „GitHub“."
        ),
        ServiceError::Rejected(m) => format!(
            "{what}: GitHub lehnt die Anfrage ab (422){}",
            if m.is_empty() {
                String::new()
            } else {
                format!(": {m}")
            }
        ),
        other => format!("{what}: {other}"),
    }
}

impl AppState {
    /// The client for the GitHub of the person (sign-in via OAuth or token). Blocking (.env file).
    pub(crate) fn github_client(&self) -> Result<GitHubClient, String> {
        let token = self.github_access().ok_or_else(|| {
            "GitHub ist nicht verbunden. Melde dich unter „Einrichten“ › „GitHub“ an.".to_string()
        })?;
        let sources = self.sources();
        let api = sources
            .github
            .as_ref()
            .map_or(DEFAULT_GITHUB_API, |g| g.api_url.as_str());
        GitHubClient::new(api, token).map_err(|e| format!("Der GitHub-Zugang ist ungültig: {e}"))
    }

    pub(crate) async fn github_client_async(&self) -> Result<GitHubClient, String> {
        self.oauth_refresh().await;
        let s = self.clone();
        tokio::task::spawn_blocking(move || s.github_client())
            .await
            .map_err(|_| "Interner Fehler".to_string())?
    }

    /// Reads the inputs of a follow-up job from GitHub. `Ok(None)` for jobs that need none
    /// (a normal run). An empty result is an error in plain words – a job without input would only
    /// burn money.
    pub(crate) async fn followup_job(
        &self,
        ws: &Workspace,
        kind: &RunKind,
    ) -> Result<Option<FollowupJob>, String> {
        let RunKind::Followup { review, task } = kind else {
            return Ok(None);
        };
        let link = ws.sync.pr(review).ok_or_else(|| {
            format!("Der Pull Request {review} ist noch nicht mit GitHub abgeglichen. Gleiche zuerst ab.")
        })?;
        // The pull request says which repo it lives in; the agent must be allowed to work there.
        let repo = link.repo.clone();
        let number = link.number;
        if !self.agent_repos().allows(&repo) {
            return Err(format!(
                "Der Pull Request {review} gehört zu {repo}, der Agent darf dort aber nicht arbeiten. Trage das Repo unter „Einrichten“ › „GitHub“ ein."
            ));
        }
        let client = self.github_client_async().await?;
        match task {
            FollowupTask::ReviewThreads => {
                let input = client.review_input(&repo, number).await.map_err(|e| {
                    github_message("Die Review-Kommentare lassen sich nicht lesen", &e)
                })?;
                if input.is_empty() {
                    return Err(format!(
                        "Zu #{review} gibt es keine Review-Kommentare, die der Agent umsetzen könnte."
                    ));
                }
                Ok(Some(FollowupJob {
                    kind: task.id(),
                    review: number,
                    repo: repo.clone(),
                    threads: input.threads,
                    notes: input.notes,
                    checks: Vec::new(),
                }))
            }
            FollowupTask::FailedChecks { .. } => {
                let checks = client
                    .failed_checks(&repo, number)
                    .await
                    .map_err(|e| github_message("Die Checks lassen sich nicht lesen", &e))?;
                if checks.is_empty() {
                    return Err(format!(
                        "Bei GitHub ist in #{review} gerade kein Check rot (vielleicht läuft schon ein neuer Durchlauf). Gleiche ab und versuche es erneut."
                    ));
                }
                Ok(Some(FollowupJob {
                    kind: task.id(),
                    review: number,
                    repo: repo.clone(),
                    threads: Vec::new(),
                    notes: Vec::new(),
                    checks,
                }))
            }
        }
    }

    /// Before a follow-up job is commissioned: does GitHub have input for it?
    pub(crate) async fn followup_precheck(&self, action: &Action) -> Result<(), ActionFailure> {
        let (review, task) = match action {
            Action::AddressReviewThreads { review, .. } => {
                (review.clone(), FollowupTask::ReviewThreads)
            }
            Action::FixFailedChecks { review, .. } => (
                review.clone(),
                FollowupTask::FailedChecks { checks: Vec::new() },
            ),
            _ => return Ok(()),
        };
        let ws = self.inner.live.copy();
        self.followup_job(&ws, &RunKind::Followup { review, task })
            .await
            .map(|_| ())
            .map_err(ActionFailure::Plan)
    }

    // ---------- Pull request ----------

    /// The finished real run behind `key`, or the sentence why there is no pull request to create.
    fn pull_request_run(&self, ws: &Workspace, key: &str) -> Result<(), String> {
        ws.check_pull_request(key).map_err(|e| e.message())
    }

    /// `GET /api/agent/pull-request?run=<key>`: title, text and branches as they would be created,
    /// and the open pull request that already exists for the branch. Creates nothing.
    pub(crate) async fn pull_request_preview(
        &self,
        key: &str,
    ) -> Result<PullRequestPreview, ActionFailure> {
        let ws = self.inner.live.copy();
        let draft = ws
            .pull_request_draft(key)
            .ok_or_else(|| ActionFailure::Plan("Diesen Lauf gibt es nicht.".into()))?;
        let run = ws.runs.iter().find(|r| r.key() == key);
        // The pull request goes into the repo the run worked in – not into "the" configured repo.
        let repo = run
            .ok_or_else(|| ActionFailure::Plan("Diesen Lauf gibt es nicht.".into()))
            .and_then(|r| self.run_repo(r).map_err(ActionFailure::Plan))?;
        let mut preview = PullRequestPreview {
            repo: repo.clone(),
            head: draft.head.clone(),
            base: draft.base,
            title: draft.title,
            body: draft.body,
            draft: run.is_some_and(|r| r.options.draft_pr),
            existing: run.and_then(|r| r.pull_request.clone()),
        };
        if preview.existing.is_some() {
            return Ok(preview);
        }
        self.pull_request_run(&ws, key)
            .map_err(ActionFailure::Plan)?;
        let client = self
            .github_client_async()
            .await
            .map_err(ActionFailure::Plan)?;
        let found = client
            .pull_for_branch(&repo, &draft.head)
            .await
            .map_err(|e| {
                ActionFailure::Service(github_message("GitHub liefert den Pull Request nicht", &e))
            })?;
        preview.existing = found.map(|p| link(&p, true));
        Ok(preview)
    }

    /// `Action::CreatePullRequest`: look for an existing pull request, otherwise create one, then
    /// book the answer of GitHub on the run. Runs only for your action; the rules of `cockpit-core`
    /// (only you, finished real run, no second pull request) are checked first.
    pub(crate) async fn pull_request_create(
        &self,
        action: Action,
    ) -> Result<cockpit_core::api::ActionResponse, ActionFailure> {
        let Action::CreatePullRequest {
            run, title, body, ..
        } = action
        else {
            return Err(ActionFailure::Intern);
        };
        let _lock = self.inner.lock.lock().await;
        // Trial on a copy: all rules apply before GitHub is asked. (The answer of the browser for
        // `pull` is never used.)
        let before = self.inner.live.copy();
        let mut trial = before.clone();
        let placeholder = Action::CreatePullRequest {
            run: run.clone(),
            title: title.clone(),
            body: body.clone(),
            pull: Some(PullRequestRef {
                number: 1,
                ..Default::default()
            }),
        };
        trial
            .apply(placeholder, Actor::User, now_ms())
            .map_err(ActionFailure::Rule)?;
        let draft = before
            .pull_request_draft(&run)
            .ok_or(ActionFailure::Intern)?;
        let as_draft = before
            .runs
            .iter()
            .find(|r| r.key() == run)
            .is_some_and(|r| r.options.draft_pr);
        let repo = before
            .runs
            .iter()
            .find(|r| r.key() == run)
            .ok_or(ActionFailure::Intern)
            .and_then(|r| self.run_repo(r).map_err(ActionFailure::Plan))?;
        let client = self
            .github_client_async()
            .await
            .map_err(ActionFailure::Plan)?;
        let service = |e: ServiceError| {
            tracing::warn!("Pull Request: {e}");
            ActionFailure::Service(format!(
                "{} Es wurde nichts geändert.",
                github_message("Der Pull Request wurde nicht angelegt", &e)
            ))
        };
        // Idempotent: an open pull request for the branch is linked, not duplicated.
        let mut found = client
            .pull_for_branch(&repo, &draft.head)
            .await
            .map_err(service)?
            .map(|p| link(&p, true));
        if found.is_none() {
            match client
                .pull_create(&repo, &draft.head, &draft.base, &title, &body, as_draft)
                .await
            {
                Ok(p) => {
                    self.inner
                        .written
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    found = Some(link(&p, false));
                }
                // Someone was faster between look-up and creation: link theirs.
                Err(ServiceError::Rejected(m)) if m.contains("already exists") => {
                    found = client
                        .pull_for_branch(&repo, &draft.head)
                        .await
                        .map_err(service)?
                        .map(|p| link(&p, true));
                    if found.is_none() {
                        return Err(service(ServiceError::Rejected(m)));
                    }
                }
                Err(e) => return Err(service(e)),
            }
        }
        let pull = found.ok_or(ActionFailure::Intern)?;
        let applied = Action::CreatePullRequest {
            run,
            title,
            body,
            pull: Some(pull),
        };
        let s = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            s.inner
                .live
                .apply(&s.inner.db, applied, Actor::User, now_ms())
        })
        .await
        .unwrap_or_else(|_| Err(LiveError::Save("Interner Fehler".into())));
        match result {
            Ok(answer) => {
                // The new pull request should show up as a review soon.
                let s = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = s.sync().await {
                        tracing::warn!("Abgleich nach dem Anlegen des Pull Requests: {e:?}");
                    }
                });
                Ok(answer)
            }
            Err(LiveError::Action(e)) => Err(ActionFailure::Service(format!(
                "Bei GitHub erledigt, lokal aber nicht übernommen ({e}). Der nächste Abgleich holt es nach."
            ))),
            Err(LiveError::Save(m)) => Err(ActionFailure::Save(m)),
        }
    }
}

fn link(p: &PullSummary, existed: bool) -> PullRequestRef {
    PullRequestRef {
        number: p.number,
        url: p.html_url.clone(),
        existed,
    }
}
