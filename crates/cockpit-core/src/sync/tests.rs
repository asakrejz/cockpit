use super::*;
use crate::action::{Action, Actor};
use crate::model::{Comment, Kind, Person, PrRef, ReasonKind, Scope};
use crate::review::{Decision, LineKind, Suggestion, Verdict};
use crate::{notify, persist};

mod changes;
mod items;
mod merge;
mod sprints;
mod two_repos;

const T: u64 = 1_800_000_000_000;

fn ticket(key: &str, title: &str) -> Item {
    Item {
        id: key.to_lowercase(),
        kind: Kind::Ticket,
        source: Source::Jira,
        key: key.into(),
        title: title.into(),
        description: String::new(),
        reason: ReasonKind::Assigned,
        reason_text: "Dir zugewiesen".into(),
        time: "vor 5 Min.".into(),
        scope: Scope::Mine,
        owner: Person::me(),
        status: TicketStatus::Open,
        branch: None,
        pr: None,
        agent_note: None,
        comments: Vec::new(),
        read: false,
        due: None,
        priority: Default::default(),
        repo: None,
        labels: Vec::new(),
        sprint: None,
        updated_ms: None,
    }
}

fn pr_item(number: u32) -> Item {
    Item {
        id: format!("gh-a-x-{number}"),
        kind: Kind::PullRequest,
        source: Source::GitHub,
        key: format!("#{number}"),
        title: format!("PR {number}"),
        reason: ReasonKind::ReviewRequested,
        reason_text: "Review angefragt".into(),
        pr: Some(PrRef {
            repo: String::new(),
            number,
            draft: false,
            checks: Checks::Passed,
            changes_requested: false,
            merged: false,
            review_requested: true,
            author: String::new(),
            mine: false,
            requested_me: false,
            assigned_me: false,
        }),
        branch: Some("feature/x".into()),
        ..ticket("X-1", "")
    }
}

fn files(lines: &[(u32, LineKind, &str)]) -> Vec<FileDiff> {
    vec![FileDiff::new("a.rs", lines)]
}

fn review(number: u32, files: Vec<FileDiff>) -> Review {
    Review {
        repo: String::new(),
        number,
        title: format!("PR {number}"),
        description: String::new(),
        author: Person::new("Lena", "LK"),
        branch: "feature/x".into(),
        base: "main".into(),
        ticket_key: None,
        checks: Checks::Passed,
        files,
        agent_reviewed: false,
        agent_summary: String::new(),
        suggestions: Vec::new(),
        line_comments: Vec::new(),
        verdict: Verdict::default(),
        sent: false,
        check_runs: Vec::new(),
        merged: None,
    }
}

fn jira(items: Vec<Item>) -> Remote {
    Remote {
        sources: vec![SourceData {
            source: Source::Jira,
            outcome: Outcome::Loaded(Loaded {
                items,
                ..Loaded::default()
            }),
        }],
    }
}

fn github(items: Vec<Item>, reviews: Vec<Review>) -> Remote {
    let prs = reviews
        .iter()
        .map(|r| PrLink {
            number: r.number,
            repo: "a/x".into(),
            head_sha: "abc".into(),
        })
        .collect();
    Remote {
        sources: vec![SourceData {
            source: Source::GitHub,
            outcome: Outcome::Loaded(Loaded {
                items,
                reviews,
                prs,
                warnings: Vec::new(),
            }),
        }],
    }
}

fn synced(remote: &Remote) -> Workspace {
    merge(&Workspace::default(), remote, T).0
}
