//! The state of the agent page: all signals in one `Copy` struct that the sections receive.

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
            tests_allowed,
        } = self;
        AgentOptions {
            run_tests: run_tests.get() && tests_allowed.get(),
            draft_pr: draft_pr.get(),
            hint: hint.get(),
            plan_first: plan_first.get(),
            repo: Some(repo.get()).filter(|r| !r.is_empty()),
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
    pub routed: Memo<cockpit_core::RepoChoice>,
    pub repo_options: Signal<Vec<(String, String)>>,
    pub note: RwSignal<String>,
    pub run: Memo<Option<AgentRun>>,
    pub steps: Memo<Vec<cockpit_core::Step>>,
    pub idle: Signal<bool>,
    pub running: Signal<bool>,
    pub awaiting: Signal<bool>,
    pub plan: Memo<Option<cockpit_core::Plan>>,
    pub asking: Signal<bool>,
    pub paused: Signal<bool>,
    pub diagnosed: Signal<bool>,
    pub followup: Signal<bool>,
    pub stopped: Signal<bool>,
    pub working: Signal<bool>,
    pub problem: RwSignal<Option<String>>,
    pub violations: Memo<Vec<PathViolation>>,
    pub tests_forced: Signal<bool>,
    pub plan_forced: Signal<bool>,
    pub done: Signal<bool>,
    pub locked: Signal<bool>,
    pub blocked: Signal<Option<&'static str>>,
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
        let repo_options = Signal::derive(move || {
            let mut list = vec![(String::new(), "Repo wählen …".to_string())];
            list.extend(
                agent_repos
                    .get()
                    .repos
                    .iter()
                    .map(|r| (r.repo.clone(), r.repo.clone())),
            );
            list
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
        let diagnosed = Signal::derive(move || run.get().is_some_and(|r| r.is_diagnosed()));
        // Follow-up jobs (review comments, red check) start on the review page.
        let followup = Signal::derive(move || run.get().is_some_and(|r| r.is_followup()));
        // The agent works on the steps – then it accepts hints.
        let stopped = Signal::derive(move || run.get().is_some_and(|r| r.is_stopped()));
        let working = Signal::derive(move || {
            running.get() || asking.get() || paused.get() || diagnosed.get() || stopped.get()
        });
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
            routed,
            repo_options,
            note,
            run,
            steps,
            idle,
            running,
            awaiting,
            plan,
            asking,
            paused,
            diagnosed,
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
