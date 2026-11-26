//! Check runs and commit statuses of a commit, and what the checks of a pull request allow:
//! reading the red ones for a follow-up job and asking GitHub to run one again.

use cockpit_i18n::msg;

use super::super::dto::{Annotation, CheckRun, CheckRuns, CombinedStatus};
use super::super::followup::{self, FailedCheck};
use super::super::map;
use super::{GitHubClient, LoadedChecks};
use crate::Error;
use crate::outbound::check_again_request;

/// Repos (per base address and token) whose check runs GitHub denied, for this process. A fine-grained
/// token cannot get the permission "Checks", so asking again at every sync would only repeat the 403.
/// A restart empties it; another token (a change of the sign-in method) has another key.
static CHECKS_DENIED: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<(u64, String)>>,
> = std::sync::LazyLock::new(Default::default);

/// Does the error mean "this access may not read it" (and not "try again later")?
fn is_denied(e: &Error) -> bool {
    matches!(e, Error::NoPermission | Error::NotFound)
}
impl GitHubClient {
    /// Status of the checks for a commit.
    pub async fn checks(&self, repo: &str, sha: &str) -> Result<cockpit_core::Checks, Error> {
        Ok(map::aggregate_checks(&self.check_runs(repo, sha).await?))
    }

    /// The individual check runs of a commit.
    pub async fn check_runs(&self, repo: &str, sha: &str) -> Result<Vec<CheckRun>, Error> {
        let runs: CheckRuns = self
            .read_json(
                self.get(&format!("/repos/{repo}/commits/{sha}/check-runs"))
                    .query(&[("per_page", "100")]),
            )
            .await?;
        Ok(runs.check_runs)
    }

    /// The combined commit status of a commit (permission "Commit statuses", read).
    pub async fn commit_status(&self, repo: &str, sha: &str) -> Result<CombinedStatus, Error> {
        self.read_json(self.get(&format!("/repos/{repo}/commits/{sha}/status")))
            .await
    }

    fn checks_denied(&self, repo: &str) -> bool {
        CHECKS_DENIED
            .lock()
            .is_ok_and(|d| d.contains(&(self.scope, repo.to_string())))
    }

    fn checks_deny(&self, repo: &str) {
        if let Ok(mut d) = CHECKS_DENIED.lock() {
            d.insert((self.scope, repo.to_string()));
        }
    }

    /// The checks of a commit for the sync. A fine-grained token cannot read check runs (403/404):
    /// then the combined commit status stands in, and if that is denied too the state is
    /// `Unknown`. A denial is remembered per repo, so later calls skip the check runs. Other
    /// errors (network, rate limit) still fail.
    pub async fn checks_load(&self, repo: &str, sha: &str) -> Result<LoadedChecks, Error> {
        if !self.checks_denied(repo) {
            match self.check_runs(repo, sha).await {
                Ok(runs) => {
                    return Ok(LoadedChecks {
                        checks: map::aggregate_checks(&runs),
                        runs,
                    });
                }
                Err(e) if is_denied(&e) => self.checks_deny(repo),
                Err(e) => return Err(e),
            }
        }
        let checks = match self.commit_status(repo, sha).await {
            Ok(status) => map::status_to_checks(&status),
            Err(e) if is_denied(&e) => cockpit_core::Checks::Unknown,
            Err(e) => return Err(e),
        };
        Ok(LoadedChecks {
            runs: Vec::new(),
            checks,
        })
    }

    /// The check runs for an action on them (logs, annotations, restart). If this access cannot
    /// read checks, the error says so in plain words instead of a bare 403.
    async fn check_runs_for_action(&self, repo: &str, sha: &str) -> Result<Vec<CheckRun>, Error> {
        let denied = || Error::Coded(msg!(IntegrationsGithubChecksDenied));
        if self.checks_denied(repo) {
            return Err(denied());
        }
        self.check_runs(repo, sha).await.map_err(|e| {
            if is_denied(&e) {
                self.checks_deny(repo);
                denied()
            } else {
                e
            }
        })
    }

    /// The red checks on the current commit of the pull request with a shortened excerpt of their
    /// output and annotations – the input for „fix red checks“. Only reads. The annotations are
    /// optional: if GitHub does not deliver them, the output alone counts.
    pub async fn failed_checks(&self, repo: &str, nr: u32) -> Result<Vec<FailedCheck>, Error> {
        let pull = self.pull(repo, nr).await?;
        let runs = self.check_runs_for_action(repo, &pull.head.sha).await?;
        let mut out = Vec::new();
        for run in runs
            .iter()
            .filter(|r| followup::is_failed(r))
            .take(followup::MAX_CHECKS)
        {
            let annotations: Vec<Annotation> = self
                .read_json(
                    self.get(&format!("/repos/{repo}/check-runs/{}/annotations", run.id))
                        .query(&[("per_page", "50")]),
                )
                .await
                .unwrap_or_default();
            out.push(FailedCheck {
                name: run.name.clone(),
                excerpt: followup::excerpt(run, &annotations),
            });
        }
        Ok(out)
    }

    /// Re-requests the check with this name on the current commit of the pull request.
    pub async fn check_again_start(&self, repo: &str, nr: u32, check: &str) -> Result<(), Error> {
        let pull = self.pull(repo, nr).await?;
        let runs = self.check_runs_for_action(repo, &pull.head.sha).await?;
        let id = runs
            .iter()
            .find(|c| c.name == check)
            .map(|c| c.id)
            .ok_or_else(|| Error::Coded(msg!(IntegrationsGithubCheckGone, check = check)))?;
        self.send(&check_again_request(repo, id)).await.map(|_| ())
    }
}
