//! The repos of a run: one run can work in several repos at once.
//!
//! Each repo has its own branch (named by the repo's pattern from the same ticket and title), its
//! own base branch, rules, pull request and outcome. The primary repo (the first one) stays in the
//! fields `repo`, `rules` and `pull_request`; [`AgentRun::repo_list`] gives the full list, also for
//! states that have none.

use super::*;

/// A run works in at most this many repos.
pub const MAX_REPOS_PER_RUN: usize = 10;

/// What became of one repo of a run.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunRepoState {
    /// The run has not finished yet.
    #[default]
    Pending,
    /// The agent changed nothing here: nothing was pushed, there is no pull request.
    Unchanged,
    /// The branch is pushed; the pull request is not created yet.
    Pushed,
    /// The branch is pushed and has its pull request.
    PrCreated,
    /// The backend did not push this repo (rule or branch check failed). Others are unaffected.
    Refused,
}

/// A repo of a run with its settings (fixed at the start) and its result.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RunRepo {
    /// `owner/name`
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub base_branch: String,
    /// The branch pattern of the repo as it applied at the start.
    #[serde(default)]
    pub branch_pattern: String,
    /// Language of the pull request text of this repo.
    #[serde(default)]
    pub pr_language: cockpit_i18n::Lang,
    #[serde(default)]
    pub rules: RepoRules,
    /// The files that were pushed.
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub state: RunRepoState,
    /// Why the repo was refused; `None` otherwise.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "cockpit_i18n::opt_msg_or_text"
    )]
    pub reason: Option<cockpit_i18n::Msg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PullRequestRef>,
}

/// The repo as the backend found it when the run ended (input of [`AgentRun::book_repos`]).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RepoResult {
    pub repo: String,
    pub state: RunRepoState,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<cockpit_i18n::Msg>,
}

/// The folder name of a repo in the container: `/work/<owner>__<name>`.
pub fn repo_dir(repo: &str) -> String {
    repo.replace('/', "__")
}

impl RunRepo {
    pub fn new(repo: &str) -> Self {
        Self {
            repo: repo.into(),
            base_branch: crate::repos::DEFAULT_BASE.into(),
            branch_pattern: crate::branch::DEFAULT_PATTERN.into(),
            ..Self::default()
        }
    }

    /// The repo for a start: its own settings from the options (`targets`), or the options'
    /// common values for the first one.
    pub fn from_options(options: &AgentOptions, repo: &str, rules: RepoRules, first: bool) -> Self {
        let mut r = Self::new(repo);
        r.rules = rules;
        if let Some(t) = options.targets.iter().find(|t| t.repo == repo) {
            r.base_branch.clone_from(&t.base_branch);
            r.branch_pattern.clone_from(&t.branch_pattern);
            r.pr_language = t.pr_language;
        } else if first {
            r.base_branch.clone_from(&options.base_branch);
            r.branch_pattern.clone_from(&options.branch_pattern);
            r.pr_language = options.pr_language;
        }
        r
    }

    /// The folder in the container below `/work`.
    pub fn dir(&self) -> String {
        repo_dir(&self.repo)
    }

    pub fn pr_number(&self) -> Option<u32> {
        self.pull_request.as_ref().map(|p| p.number)
    }

    /// The branch is on the remote (pushed, with or without pull request).
    pub fn is_pushed(&self) -> bool {
        matches!(self.state, RunRepoState::Pushed | RunRepoState::PrCreated)
    }

    /// The short state as a message.
    pub fn state_msg(&self) -> cockpit_i18n::Msg {
        match self.state {
            RunRepoState::Pending => msg!(CoreReposStatePending),
            RunRepoState::Unchanged => msg!(CoreReposStateUnchanged),
            RunRepoState::Pushed => msg!(CoreReposStatePushed),
            RunRepoState::PrCreated => msg!(CoreReposStatePrCreated),
            RunRepoState::Refused => msg!(CoreReposStateRefused),
        }
    }
}

/// What the end of a run looks like when all repos are weighed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Judgement {
    /// At least one repo was pushed: the run is done. The others stay as they are.
    Done { pushed: usize },
    /// No repo has changes.
    NothingChanged,
    /// Nothing was pushed and at least one repo was refused: the first reason.
    AllRefused { reason: cockpit_i18n::Msg },
}

/// Weighs the results of all repos: one refused repo does not block the others.
pub fn judge(results: &[RepoResult]) -> Judgement {
    let pushed = results
        .iter()
        .filter(|r| matches!(r.state, RunRepoState::Pushed | RunRepoState::PrCreated))
        .count();
    if pushed > 0 {
        return Judgement::Done { pushed };
    }
    match results.iter().find(|r| r.state == RunRepoState::Refused) {
        Some(r) => {
            let reason = r.reason.clone().unwrap_or_default();
            Judgement::AllRefused {
                reason: if results.len() > 1 {
                    msg!(CoreReposRefusedOf, repo = &r.repo, reason = reason)
                } else {
                    reason
                },
            }
        }
        None => Judgement::NothingChanged,
    }
}

impl AgentRun {
    /// All repos of the run. Old states (and runs that never chose one) have no list: then the
    /// single repo of the old fields is the list.
    pub fn repo_list(&self) -> Vec<RunRepo> {
        if !self.repos.is_empty() {
            return self.repos.clone();
        }
        if self.repo.is_empty() {
            return Vec::new();
        }
        let mut r = RunRepo::new(&self.repo);
        r.base_branch.clone_from(&self.options.base_branch);
        r.branch_pattern.clone_from(&self.options.branch_pattern);
        r.pr_language = self.options.pr_language;
        r.rules = self.rules.clone();
        r.files.clone_from(&self.files);
        r.pull_request.clone_from(&self.pull_request);
        r.state = match (&self.pull_request, self.is_done()) {
            (Some(_), _) => RunRepoState::PrCreated,
            (None, true) => RunRepoState::Pushed,
            (None, false) => RunRepoState::Pending,
        };
        vec![r]
    }

    /// The run works in more than one repo.
    pub fn is_multi_repo(&self) -> bool {
        self.repos.len() > 1
    }

    /// One repo of the run.
    pub fn repo_entry(&self, repo: &str) -> Option<RunRepo> {
        self.repo_list().into_iter().find(|r| r.repo == repo)
    }

    /// The branch of the run in `repo`: named by that repo's pattern from the same ticket and title.
    pub fn branch_in(&self, repo: &str) -> String {
        if !self.target_branch.is_empty() {
            return self.target_branch.clone();
        }
        let pattern = self
            .repo_entry(repo)
            .map(|r| r.branch_pattern)
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| self.options.branch_pattern.clone());
        crate::branch::render(
            &pattern,
            &self.ticket_key,
            &self.ticket_title,
            crate::branch::BranchType::from_word(&self.options.branch_type),
        )
    }

    /// Books what the backend found per repo when the run ended. Only repos of the run count;
    /// a repo that already has a pull request is not touched.
    pub fn book_repos(&mut self, results: &[RepoResult]) {
        for res in results {
            let Some(entry) = self.repos.iter_mut().find(|r| r.repo == res.repo) else {
                continue;
            };
            if entry.pull_request.is_some() {
                continue;
            }
            entry.state = res.state;
            entry.files.clone_from(&res.files);
            entry.reason.clone_from(&res.reason);
        }
        // The field `files` shows the files of all pushed repos (prefixed in a multi run).
        if self.is_multi_repo() {
            let multi: Vec<String> = self
                .repos
                .iter()
                .filter(|r| r.is_pushed())
                .flat_map(|r| r.files.iter().map(|f| format!("{}/{f}", r.repo)))
                .collect();
            if !multi.is_empty() {
                self.files = multi;
            }
        } else if let Some(r) = self.repos.first().filter(|r| r.is_pushed()) {
            self.files.clone_from(&r.files);
        }
    }

    /// The pull request of `repo` was created (or found).
    pub(crate) fn book_pull_request(&mut self, repo: &str, pull: PullRequestRef) {
        if let Some(entry) = self.repos.iter_mut().find(|r| r.repo == repo) {
            entry.state = RunRepoState::PrCreated;
            entry.pull_request = Some(pull.clone());
        }
        if self.repo == repo || self.repo.is_empty() {
            self.pr_number = Some(pull.number);
            self.pull_request = Some(pull);
        }
    }

    /// Pull requests of the run that exist (repo, reference), primary first.
    pub fn pull_requests(&self) -> Vec<(String, PullRequestRef)> {
        let mut out: Vec<(String, PullRequestRef)> = self
            .repos
            .iter()
            .filter_map(|r| r.pull_request.clone().map(|p| (r.repo.clone(), p)))
            .collect();
        if out.is_empty()
            && let Some(p) = &self.pull_request
        {
            out.push((self.repo.clone(), p.clone()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(repo: &str, state: RunRepoState) -> RepoResult {
        RepoResult {
            repo: repo.into(),
            state,
            files: if state == RunRepoState::Pushed {
                vec!["a.txt".into()]
            } else {
                Vec::new()
            },
            reason: if state == RunRepoState::Refused {
                Some(cockpit_i18n::Msg::raw("path forbidden"))
            } else {
                None
            },
        }
    }

    fn multi() -> AgentRun {
        let mut run = AgentRun::new("BAU-1", "Toggle broken");
        run.repo = "acme/app".into();
        run.repos = vec![RunRepo::new("acme/app"), RunRepo::new("acme/web")];
        run.repos[1].branch_pattern = "feature/{ticket}".into();
        run
    }

    #[test]
    fn one_refused_repo_does_not_block_the_others() {
        let r = [
            result("acme/app", RunRepoState::Pushed),
            result("acme/web", RunRepoState::Refused),
        ];
        assert_eq!(judge(&r), Judgement::Done { pushed: 1 });
    }

    #[test]
    fn nothing_pushed_is_judged_by_the_reason() {
        let unchanged = [
            result("acme/app", RunRepoState::Unchanged),
            result("acme/web", RunRepoState::Unchanged),
        ];
        assert_eq!(judge(&unchanged), Judgement::NothingChanged);
        let refused = [
            result("acme/app", RunRepoState::Unchanged),
            result("acme/web", RunRepoState::Refused),
        ];
        assert_eq!(
            judge(&refused),
            Judgement::AllRefused {
                reason: msg!(
                    CoreReposRefusedOf,
                    repo = "acme/web",
                    reason = cockpit_i18n::Msg::raw("path forbidden")
                )
            }
        );
        // A single repo keeps its plain reason.
        assert_eq!(
            judge(&[result("acme/app", RunRepoState::Refused)]),
            Judgement::AllRefused {
                reason: cockpit_i18n::Msg::raw("path forbidden")
            }
        );
        assert_eq!(judge(&[]), Judgement::NothingChanged);
    }

    #[test]
    fn booking_keeps_each_repo_apart() {
        let mut run = multi();
        run.book_repos(&[
            result("acme/app", RunRepoState::Pushed),
            result("acme/web", RunRepoState::Refused),
            result("acme/other", RunRepoState::Pushed),
        ]);
        assert_eq!(run.repos[0].state, RunRepoState::Pushed);
        assert_eq!(run.repos[1].state, RunRepoState::Refused);
        assert_eq!(
            run.repos[1].reason,
            Some(cockpit_i18n::Msg::raw("path forbidden"))
        );
        assert_eq!(run.files, ["acme/app/a.txt"]);
        assert_eq!(
            run.repos.len(),
            2,
            "a repo that is not part of the run is ignored"
        );
    }

    #[test]
    fn the_branch_follows_the_pattern_of_each_repo() {
        let run = multi();
        assert_eq!(run.branch_in("acme/app"), "claude/BAU-1-toggle-broken");
        assert_eq!(run.branch_in("acme/web"), "feature/BAU-1");
    }

    #[test]
    fn a_pull_request_changes_only_its_repo() {
        let mut run = multi();
        run.book_repos(&[
            result("acme/app", RunRepoState::Pushed),
            result("acme/web", RunRepoState::Pushed),
        ]);
        let pull = PullRequestRef {
            number: 7,
            ..Default::default()
        };
        run.book_pull_request("acme/web", pull.clone());
        assert_eq!(run.repos[1].state, RunRepoState::PrCreated);
        assert_eq!(run.repos[0].state, RunRepoState::Pushed);
        assert_eq!(run.pull_request, None, "the primary repo has none yet");
        run.book_pull_request("acme/app", pull);
        assert_eq!(run.pr_number, Some(7));
        assert_eq!(run.pull_requests().len(), 2);
        // Booking again does not overwrite a repo that has its pull request.
        run.book_repos(&[result("acme/app", RunRepoState::Unchanged)]);
        assert_eq!(run.repos[0].state, RunRepoState::PrCreated);
    }

    #[test]
    fn an_old_run_with_one_repo_becomes_a_list() {
        let json = r#"{"ticket_key":"BAU-1","repo":"acme/app","state":"Done","pull_request":{"number":3}}"#;
        let run: AgentRun = serde_json::from_str(json).unwrap();
        assert!(run.repos.is_empty());
        let list = run.repo_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].repo, "acme/app");
        assert_eq!(list[0].state, RunRepoState::PrCreated);
        assert_eq!(list[0].pr_number(), Some(3));
        let empty: AgentRun = serde_json::from_str(r#"{"ticket_key":"BAU-2"}"#).unwrap();
        assert!(empty.repo_list().is_empty());
    }

    #[test]
    fn options_with_one_repo_still_read() {
        let o: AgentOptions = serde_json::from_str(r#"{"repo":"acme/app"}"#).unwrap();
        assert_eq!(o.chosen_repos(), ["acme/app"]);
        let o: AgentOptions =
            serde_json::from_str(r#"{"repo":"acme/app","repos":["acme/web","acme/app"]}"#).unwrap();
        assert_eq!(o.chosen_repos(), ["acme/web", "acme/app"]);
        assert_eq!(repo_dir("acme/app"), "acme__app");
    }
}
