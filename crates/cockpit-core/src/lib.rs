//! Domain logic of the cockpit – without UI and without network.
//!
//! * [`model`]: tickets, pull requests and why they need attention
//! * [`attention`]: rules why an entry needs attention, and snoozing
//! * [`review`]: diff, suggestions of the agent, approval and verdict
//! * [`agent`]: the run of an agent as a state machine
//! * [`diagnosis`]: cause and fix proposal for a red check
//! * [`connections`]: connections, permissions and input validation
//! * [`setup`]: steps, inputs and summary of the setup assistant
//! * [`link`]: link ticket and pull request via the ticket key
//! * [`search`]: search and filter of the inbox
//! * [`notice`]: plain-language banners for sources that are not healthy, and failure texts
//! * [`notify`]: events for notifications, channels and quiet hours
//! * [`usage`]: consumption, costs, limits and the ledger of the agents
//! * [`rules`]: rules per repository (paths, required tests, instructions)
//! * [`sync`]: sync with Jira and GitHub (pure merging)
//! * [`team`]: load per person and what is currently stuck
//! * [`workspace`]: everything together, with the actions of the UI
//! * [`action`]: [`Action`], `Workspace::apply` and the log
//! * [`demo`]: sample data for tests, stories and the server's seed `COCKPIT_SEED=demo`
//! * [`api`]: the JSON shapes between app and backend (server mode)
//! * [`persist`]: saving and loading the workspace (JSON with version number)
//!
//! Every action of the UI is an [`Action`] that `Workspace::apply` executes via methods
//! on [`Workspace`] and logs – and is thus testable with `cargo test`. A real backend would run the same actions against Jira,
//! GitHub and the agent and write the result into the workspace.

pub mod action;
pub mod agent;
pub mod api;
pub mod attention;
pub mod connections;
pub mod demo;
pub mod diagnosis;
pub mod filters;
pub mod link;
pub mod model;
pub mod notice;
pub mod notify;
pub mod persist;
pub mod repos;
pub mod review;
pub mod rules;
pub mod search;
pub mod setup;
pub mod sync;
pub mod team;
pub mod text;
pub mod usage;
pub mod workspace;

pub use action::{Action, ActionError, Actor, Applied, LogEntry, MAX_LOG, Target};
pub use agent::{
    AgentOptions, AgentPlan, AgentReport, AgentRun, FollowupTask, MessageKind, Plan,
    PullRequestRef, Question, ReportEffect, RunKind, RunMessage, RunState, Speaker, Step,
    StepState, Tick,
};
pub use attention::{Hit, Rule, Rules, Snooze, SnoozeChoice};
pub use connections::{AuthKind, Connections, ServiceStatus};
pub use diagnosis::{Diagnosis, Fix};
pub use filters::{
    ASSIGNED_TO_ME, ActiveFilter, Applies, Choices, FILTERS, FilterDef, JIRA_PROJECT_FILTER,
    NO_TICKET, Ownership, RECENT_DAYS, SprintView, UNASSIGNED, UNASSIGNED_NAME, filter_def,
};
pub use model::*;
pub use notice::{NoticeAction, NoticeTone, SourceNotice, source_notices};
pub use notify::{
    Channel, Channels, Delivery, Event as NotifyEvent, NotifyReason, NotifySettings, QuietHours,
};
pub use repos::{AgentRepo, AgentRepos, RepoChoice};
pub use review::{
    CHECK_STEPS, CheckRun, Decision, DiffLine, FileDiff, Level, LineComment, LineKind,
    MergeBlocker, MergeMethod, Review, SendError, Suggestion, SuggestionDiff, Verdict,
    merge_blockers,
};
pub use rules::{DEMO_REPO, PathViolation, RepoRules, ViolationKind, glob_match};
pub use search::InboxFilter;
pub use sync::{
    Change, Loaded, Outcome, PrLink, Problem, Remote, SourceData, SourceState, SyncState,
};
pub use team::{CAPACITY, LoadLevel, Member, Stuck, StuckKind, state_label};
pub use usage::{AgentLimits, Charge, Pricing, StopReason, Usage};
pub use workspace::{
    AgentError, LinkError, PullRequestDraft, ResumeError, StatusChange, Workspace,
};
