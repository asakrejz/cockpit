//! Domain logic of the cockpit – without UI and without network.
//!
//! * [`model`]: tickets, pull requests and why they need attention
//! * [`attention`]: rules why an entry needs attention, and snoozing
//! * [`review`]: diff, suggestions of the agent, approval and verdict
//! * [`agent`]: the run of an agent as a state machine
//! * [`connections`]: the sign-in state of Jira and GitHub
//! * [`setup`]: steps, inputs and summary of the setup assistant
//! * [`link`]: link ticket and pull request via the ticket key
//! * [`search`]: text search over the inbox
//! * [`filters`]: the inbox filters (definitions, values and matching)
//! * [`notice`]: plain-language banners for sources that are not healthy, and failure texts
//! * [`notify`]: events for notifications, channels and quiet hours
//! * [`usage`]: consumption, costs, limits and the ledger of the agents
//! * [`rules`]: rules per repository (paths, required tests, instructions)
//! * [`sync`]: sync with Jira and GitHub (pure merging)
//! * [`team`]: load per person and what is currently stuck
//! * [`team_filter`]: „Mein Team“, the choice of Jira teams the lists are limited to
//! * [`workspace`]: everything together, with the actions
//! * [`action`]: [`Action`], `Workspace::apply` and the log
//! * [`demo`]: sample data for tests, stories and the server's seed `COCKPIT_SEED=demo`
//! * [`api`]: the JSON shapes between app and backend (server mode)
//! * [`persist`]: saving and loading the workspace (JSON with version number)

pub mod action;
pub mod adf;
pub mod agent;
pub mod api;
pub mod attention;
pub mod branch;
pub mod connections;
pub mod demo;
pub mod details;
pub mod filters;
pub mod language;
pub mod link;
pub mod model;
pub mod notice;
pub mod notify;
pub mod people_link;
pub mod people_search;
pub mod permissions;
pub mod persist;
pub mod project;
pub mod repos;
pub mod review;
pub mod review_filter;
pub mod rules;
pub mod search;
pub mod setup;
pub mod sync;
pub mod team;
pub mod team_filter;
#[cfg(test)]
mod test_support;
pub mod text;
pub mod usage;
pub mod workspace;

pub use action::{Action, ActionError, Actor, Applied, LogEntry, MAX_LOG, Target};
pub use agent::{
    AgentOptions, AgentPlan, AgentReport, AgentRun, Closure, FollowupTask, Judgement,
    MAX_REPOS_PER_RUN, MessageKind, Plan, PullRequestRef, Question, RepoResult, RepoTarget,
    ReportEffect, RunKind, RunMessage, RunRepo, RunRepoState, RunState, Speaker, Step, StepState,
    judge, repo_dir,
};
pub use attention::{Hit, Rule, Rules, Snooze, SnoozeChoice};
pub use connections::{AuthChoice, AuthKind, OAuthTokens, ServiceStatus};
pub use details::{Attachment, DetailField, DetailValue, Details};
pub use filters::{
    ASSIGNED_TO_ME, ActiveFilter, Applies, Choices, FILTERS, FilterDef, InboxFilter,
    JIRA_PROJECT_FILTER, NO_TICKET, RECENT_DAYS, SprintView, UNASSIGNED, filter_def,
};
pub use model::*;
pub use notice::{NoticeAction, NoticeTone, SourceNotice, source_notices};
pub use notify::{
    Channel, Channels, Delivery, Event as NotifyEvent, NotifyReason, NotifySettings, QuietHours,
};
pub use people_link::{PeopleSource, PersonLink};
pub use permissions::{Access, Right, TokenKind};
pub use project::{
    CreatedIssue, FieldDef, FieldInput, FieldKind, FieldValue, IssueLink, IssueRef, ProjectItem,
    ProjectMeta, ProjectOrigin, ProjectRef, ProjectSummary, SelectOption, TicketsFrom,
};
pub use repos::{AgentRepo, AgentRepos};
pub use review::{
    AgentComment, CheckRun, Decision, DiffLine, DiffSide, FileDiff, Level, LineComment, LineKind,
    MAX_AGENT_COMMENTS, MergeBlocker, MergeMethod, Review, SHOWN_LINES, SendError, Suggestion,
    SuggestionDiff, TreeRow, Verdict, Viewed, commit_date, file_tree, merge_blockers, short_sha,
};
pub use review_filter::{Ownership, ProjectChoice, RepoGroup, ReviewFilter};
pub use rules::{PathViolation, RepoRules, ViolationKind, glob_match};
pub use sync::{
    Change, Loaded, Outcome, PrLink, Problem, Remote, SourceData, SourceState, SyncState,
};
pub use team::{CAPACITY, LoadLevel, Member, Stuck, StuckKind, state_tone};
pub use team_filter::TeamChoice;
pub use usage::{AgentLimits, Charge, Pricing, StopReason, Usage};
pub use workspace::{AgentError, LinkedPr, PullRequestDraft, ResumeError, StatusChange, Workspace};
