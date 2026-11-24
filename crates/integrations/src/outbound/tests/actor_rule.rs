//! The rule behind every outward call: only an action that requires `Actor::User` may become
//! one. `plane` is the single place where actions turn into calls, so this test walks through
//! every action there is.

use super::*;
use cockpit_core::project::{FieldDef, FieldKind, ProjectItem, ProjectMeta, SelectOption};
use cockpit_core::{
    AgentLimits, AgentOptions, AgentReport, Channel, Closure, FieldInput, InboxFilter,
    NotifyReason, PeopleSource, Priority, QuietHours, RepoRules, ReviewFilter, Rule, TeamChoice,
};

/// How many variants `Action` has. See [`slot`].
const VARIANTS: usize = 60;

/// Gives every variant its own number. The `match` has no wildcard arm on purpose: a new variant
/// does not compile until it is added here, and then [`every_action`] has to contain it (the test
/// `the_list_holds_every_variant` fails otherwise).
fn slot(action: &Action) -> usize {
    match action {
        Action::MarkDone { .. } => 0,
        Action::SetStatus { .. } => 1,
        Action::CreateBranch { .. } => 2,
        Action::Reply { .. } => 3,
        Action::Snooze { .. } => 4,
        Action::Unsnooze { .. } => 5,
        Action::WakeSnoozed => 6,
        Action::SetRule { .. } => 7,
        Action::EditTicket { .. } => 8,
        Action::AssignTicket { .. } => 9,
        Action::SetPriority { .. } => 10,
        Action::AutoLinkPrs => 11,
        Action::RequestReview { .. } => 12,
        Action::SyncTicketStatus => 13,
        Action::SetStatusFollowsPr { .. } => 14,
        Action::SelectJiraProject { .. } => 15,
        Action::SetTeamChoice { .. } => 16,
        Action::SetPeopleSource { .. } => 17,
        Action::LinkPeople { .. } => 18,
        Action::UnlinkPeople { .. } => 19,
        Action::SetInboxFilter { .. } => 20,
        Action::SetReviewFilter { .. } => 21,
        Action::SetLabel { .. } => 22,
        Action::SetIssueClosed { .. } => 23,
        Action::SetProjectField { .. } => 24,
        Action::CreateIssue { .. } => 25,
        Action::ConvertDraft { .. } => 26,
        Action::Decide { .. } => 27,
        Action::EditSuggestion { .. } => 28,
        Action::AddLineComment { .. } => 29,
        Action::EditLineComment { .. } => 30,
        Action::DeleteLineComment { .. } => 31,
        Action::SetVerdict { .. } => 32,
        Action::SendReview { .. } => 33,
        Action::RestartCheck { .. } => 34,
        Action::MergePr { .. } => 35,
        Action::StartAgent { .. } => 36,
        Action::CancelAgent { .. } => 37,
        Action::ApprovePlan { .. } => 38,
        Action::RejectPlan { .. } => 39,
        Action::RevisePlan { .. } => 40,
        Action::AnswerAgent { .. } => 41,
        Action::SteerAgent { .. } => 42,
        Action::PauseAgent { .. } => 43,
        Action::ResumeAgent { .. } => 44,
        Action::ReportAgent { .. } => 45,
        Action::CloseAgentRun { .. } => 46,
        Action::RemoveAgentRun { .. } => 47,
        Action::RemoveFinishedRuns => 48,
        Action::StartPrReview { .. } => 49,
        Action::AddressReviewThreads { .. } => 50,
        Action::FixFailedChecks { .. } => 51,
        Action::CreatePullRequest { .. } => 52,
        Action::ResumeStopped { .. } => 53,
        Action::SetLanguage { .. } => 54,
        Action::SetAgentLimits { .. } => 55,
        Action::SetRepoRules { .. } => 56,
        Action::SetNotifyChannel { .. } => 57,
        Action::SetQuietHours { .. } => 58,
        Action::SyncRemote { .. } => 59,
    }
}

/// The calls `plane` can produce, by name. Exhaustive, so a new kind of call is noticed here.
fn kind(call: &Outbound) -> &'static str {
    match call {
        Outbound::JiraComment { .. } => "JiraComment",
        Outbound::JiraStatus { .. } => "JiraStatus",
        Outbound::GitHubComment { .. } => "GitHubComment",
        Outbound::GitHubBranch { .. } => "GitHubBranch",
        Outbound::GitHubReview { .. } => "GitHubReview",
        Outbound::GitHubMerge { .. } => "GitHubMerge",
        Outbound::GitHubRecheck { .. } => "GitHubRecheck",
        Outbound::ProjectField { .. } => "ProjectField",
        Outbound::DraftEdit { .. } => "DraftEdit",
        Outbound::Issue { .. } => "Issue",
    }
}

const KINDS: [&str; 10] = [
    "JiraComment",
    "JiraStatus",
    "GitHubComment",
    "GitHubBranch",
    "GitHubReview",
    "GitHubMerge",
    "GitHubRecheck",
    "ProjectField",
    "DraftEdit",
    "Issue",
];

/// A Jira ticket, a pull request with its ticket, and an issue and a draft of a GitHub project,
/// all synced, so that a service would be called for each of them.
fn ws_with_project() -> Workspace {
    let mut issue = item("ghi-a-x-12", "a/x#12", Source::GitHubProject);
    issue.kind = Kind::Ticket;
    issue.repo = Some("a/x".into());
    issue.project = Some(ProjectItem {
        project_id: "PVT_1".into(),
        project: "acme/1".into(),
        item_id: "PVTI_12".into(),
        content_id: "I_12".into(),
        number: Some(12),
        ..ProjectItem::default()
    });
    let mut draft = item("ghd-1", "acme/1#draft", Source::GitHubProject);
    draft.kind = Kind::Ticket;
    draft.project = Some(ProjectItem {
        project_id: "PVT_1".into(),
        project: "acme/1".into(),
        item_id: "PVTI_13".into(),
        content_id: "DI_13".into(),
        draft: true,
        ..ProjectItem::default()
    });
    let select = |id: &str, name: &str, options: &[(&str, &str)]| FieldDef {
        id: id.into(),
        name: name.into(),
        kind: FieldKind::SingleSelect,
        options: options
            .iter()
            .map(|(id, name)| SelectOption {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect(),
    };
    let meta = ProjectMeta {
        id: "PVT_1".into(),
        owner: "acme".into(),
        number: 1,
        title: "Board".into(),
        url: String::new(),
        fields: vec![
            select(
                "F_status",
                "Status",
                &[("o_todo", "Todo"), ("o_review", "In Review")],
            ),
            select(
                "F_prio",
                "Priority",
                &[("o_high", "High"), ("o_low", "Low")],
            ),
        ],
        status_field: None,
    };
    let mut remote = remote();
    remote.sources.push(SourceData {
        source: Source::GitHubProject,
        outcome: Outcome::Loaded(Loaded {
            items: vec![issue, draft],
            projects: vec![meta],
            ..Loaded::default()
        }),
    });
    cockpit_core::sync::merge(&Workspace::default(), &remote, T).0
}

fn with_projects() -> Targets {
    Targets {
        projects: true,
        ..all()
    }
}

/// At least one value of every variant, with the items of [`ws_with_project`] where the action
/// would reach a service. A variant that works on several kinds of items appears once per kind.
fn every_action() -> Vec<Action> {
    let jira = || "abc-1".to_string();
    let pull = || "gh-a-x-7".to_string();
    let issue = || "ghi-a-x-12".to_string();
    let draft = || "ghd-1".to_string();
    let pr = || PrKey::from(7);
    let options = AgentOptions::default;
    vec![
        // Inbox
        Action::MarkDone { item: jira() },
        Action::SetStatus {
            item: jira(),
            status: TicketStatus::Review,
        },
        Action::SetStatus {
            item: issue(),
            status: TicketStatus::Review,
        },
        Action::CreateBranch { item: jira() },
        Action::Reply {
            item: jira(),
            text: "Hello".into(),
        },
        Action::Reply {
            item: pull(),
            text: "Hello".into(),
        },
        Action::Reply {
            item: issue(),
            text: "Hello".into(),
        },
        Action::Snooze {
            item: jira(),
            until_ms: None,
        },
        Action::Unsnooze { item: jira() },
        Action::WakeSnoozed,
        Action::SetRule {
            rule: Rule::Mention,
            enabled: false,
        },
        // Edit tickets
        Action::EditTicket {
            item: jira(),
            title: "Title".into(),
            description: "Text".into(),
        },
        Action::EditTicket {
            item: issue(),
            title: "Title".into(),
            description: "Text".into(),
        },
        Action::EditTicket {
            item: draft(),
            title: "Title".into(),
            description: "Text".into(),
        },
        Action::AssignTicket {
            item: jira(),
            person: Person::me(),
        },
        Action::AssignTicket {
            item: issue(),
            person: Person::me(),
        },
        Action::SetPriority {
            item: jira(),
            priority: Priority::High,
        },
        Action::SetPriority {
            item: issue(),
            priority: Priority::High,
        },
        // Ticket and pull request
        Action::AutoLinkPrs,
        Action::RequestReview { review: pr() },
        Action::SyncTicketStatus,
        Action::SetStatusFollowsPr { enabled: false },
        Action::SelectJiraProject { project: None },
        Action::SetTeamChoice {
            choice: TeamChoice::default(),
        },
        Action::SetPeopleSource {
            source: PeopleSource::All,
        },
        Action::LinkPeople {
            jira: "Ada".into(),
            github: "ada".into(),
        },
        Action::UnlinkPeople { jira: "Ada".into() },
        Action::SetInboxFilter {
            filter: InboxFilter::default(),
        },
        Action::SetReviewFilter {
            filter: ReviewFilter::default(),
        },
        // GitHub Projects
        Action::SetLabel {
            item: issue(),
            label: "bug".into(),
            on: true,
        },
        Action::SetIssueClosed {
            item: issue(),
            closed: true,
        },
        Action::SetProjectField {
            item: issue(),
            field_id: "F_prio".into(),
            value: Some(FieldInput::Select("o_high".into())),
        },
        Action::CreateIssue {
            project: "PVT_1".into(),
            repo: "a/x".into(),
            title: "Title".into(),
            body: String::new(),
            status: None,
            created: None,
        },
        Action::ConvertDraft {
            item: draft(),
            repo: "a/x".into(),
            created: None,
        },
        // Review
        Action::Decide {
            review: pr(),
            suggestion: "s1".into(),
            decision: Some(Decision::Approved),
        },
        Action::EditSuggestion {
            review: pr(),
            suggestion: "s1".into(),
            text: "Text".into(),
        },
        Action::AddLineComment {
            review: pr(),
            file: "a.rs".into(),
            line: 0,
            text: "Text".into(),
            suggestion: None,
        },
        Action::EditLineComment {
            review: pr(),
            comment: "c1".into(),
            text: "Text".into(),
            suggestion: None,
        },
        Action::DeleteLineComment {
            review: pr(),
            comment: "c1".into(),
        },
        Action::SetVerdict {
            review: pr(),
            verdict: Verdict::Approve,
        },
        Action::SendReview {
            review: pr(),
            comment: "Looks good".into(),
        },
        Action::RestartCheck {
            review: pr(),
            check: "build".into(),
        },
        Action::MergePr {
            review: pr(),
            method: MergeMethod::Merge,
        },
        // Agent
        Action::StartAgent {
            ticket: "ABC-1".into(),
            options: options(),
        },
        Action::CancelAgent {
            ticket: "ABC-1".into(),
        },
        Action::ApprovePlan {
            ticket: "ABC-1".into(),
        },
        Action::RejectPlan {
            ticket: "ABC-1".into(),
        },
        Action::RevisePlan {
            ticket: "ABC-1".into(),
            note: "Note".into(),
        },
        Action::AnswerAgent {
            run: "ABC-1".into(),
            text: "Text".into(),
        },
        Action::SteerAgent {
            run: "ABC-1".into(),
            text: "Text".into(),
        },
        Action::PauseAgent {
            run: "ABC-1".into(),
        },
        Action::ResumeAgent {
            run: "ABC-1".into(),
        },
        Action::ReportAgent {
            run: "ABC-1".into(),
            report: AgentReport::Started {
                detail: String::new(),
            },
        },
        Action::CloseAgentRun {
            run: "ABC-1".into(),
            closure: Closure::Merged,
        },
        Action::RemoveAgentRun {
            run: "ABC-1".into(),
        },
        Action::RemoveFinishedRuns,
        Action::StartPrReview {
            review: pr(),
            language: None,
            repo_language: None,
        },
        Action::AddressReviewThreads {
            review: pr(),
            options: options(),
        },
        Action::FixFailedChecks {
            review: pr(),
            options: options(),
        },
        Action::CreatePullRequest {
            run: "ABC-1".into(),
            title: "Title".into(),
            body: String::new(),
            pull: None,
            repo: String::new(),
        },
        Action::ResumeStopped {
            run: "ABC-1".into(),
            extra: 0,
        },
        // Settings and miscellaneous
        Action::SetLanguage {
            language: cockpit_i18n::Lang::En,
        },
        Action::SetAgentLimits {
            limits: AgentLimits::default(),
        },
        Action::SetRepoRules {
            repo: "a/x".into(),
            rules: RepoRules::default(),
        },
        Action::SetNotifyChannel {
            reason: NotifyReason::Mention,
            channel: Channel::Browser,
            on: true,
        },
        Action::SetQuietHours {
            quiet: QuietHours::default(),
        },
        Action::SyncRemote {
            remote: Remote::default(),
        },
    ]
}

#[test]
fn the_list_holds_every_variant() {
    let mut seen = [false; VARIANTS];
    for action in every_action() {
        seen[slot(&action)] = true;
    }
    let missing: Vec<usize> = (0..VARIANTS).filter(|i| !seen[*i]).collect();
    assert!(
        missing.is_empty(),
        "`every_action` has no value for the variants numbered {missing:?} in `slot`"
    );
}

#[test]
fn only_actions_that_require_the_user_become_calls() {
    let w = ws_with_project();
    let targets = with_projects();
    let mut reached: Vec<&str> = Vec::new();
    for action in every_action() {
        let planned = plan(&w, action.clone(), &targets);
        if let Ok(Some(call)) = &planned {
            reached.push(kind(call));
        }
        if action.required_actor() != Some(Actor::User) {
            assert_eq!(
                planned,
                Ok(None),
                "{action:?} does not require the user, so it must never reach a service"
            );
        }
    }
    // The test is only worth something if the data above really reaches every kind of call.
    for k in KINDS {
        assert!(reached.contains(&k), "no action in the list became a {k}");
    }
}
