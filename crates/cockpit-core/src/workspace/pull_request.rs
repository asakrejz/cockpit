//! The pull request for the branch of a finished real run: the preview text, the check that one
//! may be created, and booking it once GitHub has created it.

use cockpit_i18n::msg;

use super::*;
use crate::agent::RunRepoState;

impl Workspace {
    /// The language of the text (pull request, commit message) the agent writes for `run`: the
    /// language detected from the ticket, else `repo_lang` (the repo's `pr_language`), else English.
    pub fn pr_text_language(
        &self,
        run: &AgentRun,
        repo_lang: cockpit_i18n::Lang,
    ) -> crate::language::LangChoice {
        let description = self
            .item_by_key(&run.ticket_key)
            .map_or("", |i| i.description.as_str());
        crate::language::choose_text(&run.ticket_title, description, Some(repo_lang))
    }

    /// Title and text the pull request in `repo` would get, for the preview. `None` if the run
    /// does not exist or does not work in that repo. An empty `repo` means the primary one.
    pub fn pull_request_draft(&self, run_key: &str, repo: &str) -> Option<PullRequestDraft> {
        let run = self.runs.iter().find(|r| r.key() == run_key)?;
        let repo = if repo.is_empty() {
            run.repo.as_str()
        } else {
            repo
        };
        let entry = run.repo_entry(repo);
        if entry.is_none() && !(repo == run.repo || run.repo.is_empty()) {
            return None;
        }
        // Only the files of this repo (a multi run keeps them apart; a single run has them in `files`).
        let own: Vec<String> = match &entry {
            Some(e) if !e.files.is_empty() => e.files.clone(),
            _ => run.files.clone(),
        };
        let files: Vec<String> = own.iter().take(20).map(|f| format!("- `{f}`")).collect();
        // The text follows the language of the ticket, else the repo (the team language), never the interface.
        let repo_lang = entry
            .as_ref()
            .filter(|_| run.is_multi_repo())
            .map_or(run.options.pr_language, |e| e.pr_language);
        let lang = self.pr_text_language(run, repo_lang).lang;
        let text = |msg: cockpit_i18n::Msg| msg.render(lang);
        let summary = run.summary.clone().map(&text).unwrap_or_default();
        let mut body = format!(
            "## {}\n{}\n",
            text(cockpit_i18n::msg!(CorePrWhat)),
            summary.trim()
        );
        body.push_str(&format!(
            "\n## {}\n{}: {}\n",
            text(cockpit_i18n::msg!(CorePrTicket)),
            run.ticket_key,
            run.ticket_title
        ));
        // A GitHub issue is closed by the merge: `Fixes` is GitHub's own keyword, so it stays English
        // whatever language the text has.
        if crate::project::parse_issue_key(&run.ticket_key).is_some() {
            body.push_str(&format!("\nFixes {}\n", run.ticket_key));
        }
        if !files.is_empty() {
            body.push_str(&format!(
                "\n## {}\n{}\n",
                text(cockpit_i18n::msg!(CorePrFiles, n = own.len())),
                files.join("\n")
            ));
        }
        // The sibling pull requests of the same run: linked if they exist, otherwise the branch.
        let siblings: Vec<String> = run
            .repo_list()
            .iter()
            .filter(|r| r.repo != repo && r.is_pushed())
            .map(|r| match &r.pull_request {
                Some(p) if !p.url.is_empty() => format!("- {}#{}: {}", r.repo, p.number, p.url),
                Some(p) => format!("- {}#{}", r.repo, p.number),
                None => format!("- {}: `{}`", r.repo, run.branch_in(&r.repo)),
            })
            .collect();
        if run.is_multi_repo() && !siblings.is_empty() {
            body.push_str(&format!(
                "\n## {}\n{}\n",
                text(cockpit_i18n::msg!(CorePrSiblings)),
                siblings.join("\n")
            ));
        }
        body.push_str(&format!(
            "\n---\n{}\n",
            text(cockpit_i18n::msg!(CorePrFooter))
        ));
        let base = entry
            .as_ref()
            .map(|e| e.base_branch.clone())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| run.options.base_branch.clone());
        Some(PullRequestDraft {
            title: format!("{}: {}", run.ticket_key, run.ticket_title),
            body,
            head: run.branch_in(repo),
            base,
        })
    }

    /// May a pull request be created for this run? The run must be a finished implementation
    /// (the backend pushed its branch before reporting it done) without a pull request.
    /// With several repos the pull request is created per repo: only a repo whose branch was
    /// pushed qualifies (not an unchanged or refused one), and each gets only one.
    pub fn check_pull_request(&self, run_key: &str, repo: &str) -> Result<(), ActionError> {
        let run = self
            .runs
            .iter()
            .find(|r| r.key() == run_key)
            .ok_or(ActionError::NothingToDo)?;
        if run.kind != RunKind::Implement || !run.is_done() {
            return Err(ActionError::PullRequestNotReady);
        }
        let repo = if repo.is_empty() {
            run.repo.as_str()
        } else {
            repo
        };
        if run.repos.is_empty() {
            if let Some(p) = &run.pull_request {
                return Err(ActionError::PullRequestExists(p.number));
            }
            return Ok(());
        }
        let entry = run
            .repos
            .iter()
            .find(|r| r.repo == repo)
            .ok_or(ActionError::PullRequestNotReady)?;
        if let Some(p) = &entry.pull_request {
            return Err(ActionError::PullRequestExists(p.number));
        }
        if entry.state != RunRepoState::Pushed {
            return Err(ActionError::PullRequestNotReady);
        }
        Ok(())
    }

    /// GitHub has created the pull request (or the existing one was found): book it on the run.
    pub(crate) fn pull_request_created(&mut self, run_key: &str, repo: &str, pull: PullRequestRef) {
        let Some(run) = self.runs.iter_mut().find(|r| r.key() == run_key) else {
            return;
        };
        let ticket = run.ticket_key.clone();
        let number = pull.number;
        let repo = if repo.is_empty() {
            run.repo.clone()
        } else {
            repo.to_string()
        };
        if run.repos.is_empty() {
            run.pr_number = Some(pull.number);
            run.pull_request = Some(pull);
        } else {
            run.book_pull_request(&repo, pull);
        }
        if let Some(item) = self.items.iter_mut().find(|i| i.key == ticket) {
            item.set_note(msg!(CoreWorkspacePrCreated, number = number));
        }
    }
}

/// Title and text of a pull request that is about to be created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequestDraft {
    pub title: String,
    pub body: String,
    /// The branch of the agent.
    pub head: String,
    /// The branch the pull request goes against.
    pub base: String,
}
