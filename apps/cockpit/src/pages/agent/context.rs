//! The state of the agent page: all signals in one `Copy` struct that the sections receive.

use cockpit_core::api::GitHubRepoList;
use cockpit_core::repos::suggest::RepoSuggestion;
use cockpit_core::{AgentOptions, AgentRepos, AgentRun, Item, PathViolation};
use leptos::prelude::*;

use crate::store::{Store, use_store};

/// The switches of the job and what they need to turn into [`AgentOptions`].
#[derive(Clone, Copy)]
pub(super) struct OptionSignals {
    pub run_tests: RwSignal<bool>,
    pub draft_pr: RwSignal<bool>,
    pub hint: RwSignal<String>,
    pub plan_first: RwSignal<bool>,
    pub agent_repos: Memo<AgentRepos>,
    pub repo: RwSignal<String>,
    /// More repos of the run besides `repo` (the first one).
    pub others: RwSignal<Vec<String>>,
    pub tests_allowed: Signal<bool>,
}

impl OptionSignals {
    /// The options as the switches show them now.
    pub(super) fn chosen(self) -> AgentOptions {
        let OptionSignals {
            run_tests,
            draft_pr,
            hint,
            plan_first,
            agent_repos,
            repo,
            others,
            tests_allowed,
        } = self;
        AgentOptions {
            run_tests: run_tests.get() && tests_allowed.get(),
            draft_pr: draft_pr.get(),
            hint: hint.get(),
            plan_first: plan_first.get(),
            repo: Some(repo.get()).filter(|r| !r.is_empty()),
            repos: if repo.get().is_empty() {
                Vec::new()
            } else {
                std::iter::once(repo.get()).chain(others.get()).collect()
            },
            base_branch: agent_repos.with(|a| a.base_branch(&repo.get())),
            // Only a preview: the backend sets the pattern from its own configuration.
            branch_pattern: agent_repos.with(|a| a.branch_pattern(&repo.get())),
            ..AgentOptions::default()
        }
    }
}

/// Everything the sections of the page read and write.
#[derive(Clone, Copy)]
pub(super) struct AgentCtx {
    pub store: Store,
    pub key: StoredValue<String>,
    pub item: Memo<Option<Item>>,
    pub form: OptionSignals,
    pub repo_touched: RwSignal<bool>,
    pub project: Memo<Option<String>>,
    /// Every repo the credential can see (kept for a few minutes by the server).
    pub repo_list: RwSignal<Option<GitHubRepoList>>,
    pub repo_list_error: RwSignal<Option<String>>,
    pub repo_loading: RwSignal<bool>,
    /// Repos to offer first, each with its reason.
    pub suggestions: RwSignal<Vec<RepoSuggestion>>,
    /// The base branch of the chosen repo: its entry in the settings, otherwise the default
    /// branch GitHub reports for it.
    pub base_branch: Signal<String>,
    pub note: RwSignal<String>,
    pub run: Memo<Option<AgentRun>>,
    pub steps: Memo<Vec<cockpit_core::Step>>,
    pub idle: Signal<bool>,
    pub running: Signal<bool>,
    pub awaiting: Signal<bool>,
    pub plan: Memo<Option<cockpit_core::Plan>>,
    pub asking: Signal<bool>,
    pub paused: Signal<bool>,
    pub followup: Signal<bool>,
    pub stopped: Signal<bool>,
    pub working: Signal<bool>,
    pub problem: RwSignal<Option<String>>,
    pub violations: Memo<Vec<PathViolation>>,
    pub tests_forced: Signal<bool>,
    pub plan_forced: Signal<bool>,
    pub done: Signal<bool>,
    pub locked: Signal<bool>,
    pub blocked: Signal<Option<cockpit_i18n::Msg>>,
    pub answer: RwSignal<String>,
    pub steer: RwSignal<String>,
    pub answer_empty: Signal<bool>,
    pub steer_empty: Signal<bool>,
    pub note_empty: Signal<bool>,
}

/// The Jira project of a ticket (`BAU` for `BAU-142`).
fn item_project(store: Store, key: &str) -> Option<String> {
    store.ws.with(|w| {
        w.item_by_key(key)
            .and_then(Item::project)
            .map(str::to_string)
    })
}

impl AgentCtx {
    pub(super) fn new(ticket: Option<String>) -> Self {
        let store = use_store();
        // Without a ticket in the address: a running agent, otherwise the first ticket without a
        // pull request, otherwise the last run.
        let key = ticket.or_else(|| {
            store.ws.with_untracked(|w| {
                let running = w.runs.iter().find(|r| r.is_active()).map(|r| r.key());
                let open = w
                    .items
                    .iter()
                    .find(|i| i.is_ticket() && i.pr.is_none())
                    .map(|i| i.key.clone());
                running.or(open).or(w.runs.last().map(|r| r.key()))
            })
        });
        let key = StoredValue::new(key.unwrap_or_default());
        // The entry for the run: the ticket, for follow-up jobs the entry with the pull request.
        let item = Memo::new(move |_| {
            store.ws.with(|w| {
                let key = w
                    .run(&key.get_value())
                    .map_or_else(|| key.get_value(), |r| r.ticket_key);
                w.item_by_key(&key).cloned()
            })
        });

        // Options of the job. If the agent is already running, those of the run apply.
        let saved = store
            .ws
            .with_untracked(|w| w.run(&key.get_value()))
            .filter(|r| !r.is_idle());
        let options = saved.map(|r| r.options).unwrap_or_default();
        let others = RwSignal::new(
            options
                .chosen_repos()
                .into_iter()
                .skip(1)
                .collect::<Vec<_>>(),
        );
        let run_tests = RwSignal::new(options.run_tests);
        let draft_pr = RwSignal::new(options.draft_pr);
        let hint = RwSignal::new(options.hint);
        let plan_first = RwSignal::new(options.plan_first);
        // The repo the agent works in. Preselected by the routing rules of the configuration
        // (Jira project → repo, otherwise the default); the user can change it before the start.
        let agent_repos = Memo::new(move |_| {
            store.health.with(|h| {
                h.as_ref()
                    .map(|h| h.agent.repos.clone())
                    .unwrap_or_default()
            })
        });
        let repo = RwSignal::new(options.repo.clone().unwrap_or_default());
        let repo_touched = RwSignal::new(options.repo.is_some());
        let project = Memo::new(move |_| item_project(store, &key.get_value()));
        let routed = Memo::new(move |_| agent_repos.get().choose(project.get().as_deref()));
        Effect::new(move |_| {
            // Until the user chooses, the rule (or the default) decides.
            if !repo_touched.get_untracked() {
                repo.set(routed.get().repo().unwrap_or_default().to_string());
            }
        });
        let repo_list: RwSignal<Option<GitHubRepoList>> = RwSignal::new(None);
        let repo_list_error = RwSignal::new(None::<String>);
        let repo_loading = RwSignal::new(false);
        let suggestions: RwSignal<Vec<RepoSuggestion>> = RwSignal::new(Vec::new());
        let base_branch = Signal::derive(move || {
            let chosen = repo.get();
            // An entry that only says `main` may be the fallback of a watched repo: GitHub's own
            // default branch is the better answer then.
            let own = agent_repos
                .with(|a| a.find(&chosen).map(|r| r.base_branch.clone()))
                .filter(|b| b != cockpit_core::repos::DEFAULT_BASE);
            own.or_else(|| {
                repo_list.with(|l| {
                    l.as_ref().and_then(|l| {
                        l.repos
                            .iter()
                            .find(|r| r.repo == chosen)
                            .map(|r| r.default_branch.clone())
                            .filter(|b| !b.is_empty())
                    })
                })
            })
            .unwrap_or_else(|| agent_repos.with(|a| a.base_branch(&chosen)))
        });
        // Note for a new plan.
        let note = RwSignal::new(String::new());
        let tests_allowed = Signal::derive(move || store.conn.get().may_run_tests);
        let form = OptionSignals {
            run_tests,
            draft_pr,
            hint,
            plan_first,
            agent_repos,
            repo,
            others,
            tests_allowed,
        };

        // The run as it looks now. Before the start it already shows the chosen options.
        let run = Memo::new(move |_| {
            let mut run = store.ws.with(|w| w.run(&key.get_value()))?;
            if run.is_idle() {
                run.options = form.chosen();
            }
            Some(run)
        });
        let steps = Memo::new(move |_| run.get().map(|r| r.steps()).unwrap_or_default());
        let idle = Signal::derive(move || run.get().is_some_and(|r| r.is_idle()));
        let running = Signal::derive(move || run.get().is_some_and(|r| r.is_running()));
        let awaiting = Signal::derive(move || run.get().is_some_and(|r| r.is_awaiting()));
        let plan = Memo::new(move |_| run.get().and_then(|r| r.plan()));
        let asking = Signal::derive(move || run.get().is_some_and(|r| r.is_asking()));
        let paused = Signal::derive(move || run.get().is_some_and(|r| r.is_paused()));
        // Follow-up jobs (review comments, red check) start on the review page.
        let followup = Signal::derive(move || run.get().is_some_and(|r| r.is_followup()));
        // The agent works on the steps – then it accepts hints.
        let stopped = Signal::derive(move || run.get().is_some_and(|r| r.is_stopped()));
        let working =
            Signal::derive(move || running.get() || asking.get() || paused.get() || stopped.get());
        // Message of the last action that did not work (e.g. limit of parallel runs, rule violation).
        let problem = RwSignal::new(None::<String>);
        // The rules of the repo bind the job: required tests and plan requirement cannot be switched off.
        let rules = Memo::new(move |_| run.get().map(|r| r.rules).unwrap_or_default());
        let violations = Memo::new(move |_| run.get().map(|r| r.violations()).unwrap_or_default());
        let tests_forced = Signal::derive(move || !rules.get().required_tests.is_empty());
        let plan_forced =
            Signal::derive(move || rules.get().plan_required || !violations.get().is_empty());
        let done = Signal::derive(move || run.get().is_some_and(|r| r.is_done()));
        let locked = Signal::derive(move || !idle.get());
        let blocked = Signal::derive(move || store.conn.get().agent_blocked());

        // Before the start: ask once for the repos GitHub shows and the suggestions for this
        // ticket (the server keeps the list for a few minutes).
        let loaded = RwSignal::new(false);
        Effect::new(move |_| {
            if !idle.get() || followup.get() || loaded.get_untracked() {
                return;
            }
            loaded.set(true);
            let ticket = item
                .get_untracked()
                .map(|i| i.key)
                .unwrap_or_else(|| key.get_value());
            repo_loading.set(true);
            leptos::task::spawn_local(async move {
                match crate::server::fetch_github_repos().await {
                    Ok(l) => repo_list.set(Some(l)),
                    Err(e) => repo_list_error.set(Some(crate::pages::setup::error_text(&e))),
                }
                if let Ok(s) = crate::server::fetch_repo_suggestions(&ticket).await {
                    suggestions.set(s.suggestions);
                }
                repo_loading.set(false);
            });
        });

        // What the rules demand is already in the switches before the start.
        Effect::new(move |_| {
            if tests_forced.get() {
                run_tests.set(true);
            }
            if plan_forced.get() {
                plan_first.set(true);
            }
        });
        // Answer to a question and hint to the running agent.
        let answer = RwSignal::new(String::new());
        let steer = RwSignal::new(String::new());
        let answer_empty = Signal::derive(move || answer.with(|a| a.trim().is_empty()));
        let steer_empty = Signal::derive(move || steer.with(|a| a.trim().is_empty()));
        let note_empty = Signal::derive(move || note.with(|n| n.trim().is_empty()));

        Self {
            store,
            key,
            item,
            form,
            repo_touched,
            project,
            repo_list,
            repo_list_error,
            repo_loading,
            suggestions,
            base_branch,
            note,
            run,
            steps,
            idle,
            running,
            awaiting,
            plan,
            asking,
            paused,
            followup,
            stopped,
            working,
            problem,
            violations,
            tests_forced,
            plan_forced,
            done,
            locked,
            blocked,
            answer,
            steer,
            answer_empty,
            steer_empty,
            note_empty,
        }
    }
}
