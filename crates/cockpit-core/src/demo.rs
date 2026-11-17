//! Sample data following the design. They stand in for Jira, GitHub and the agent
//! as long as no backend is connected.

use crate::agent::{AgentRun, Question, RunState, STEPS};
use crate::model::*;
use crate::review::LineKind::{Add, Context as Ctx, Del};
use crate::review::{CheckRun, FileDiff, Level, LineComment, Review, Suggestion, Verdict};
use crate::rules::{DEMO_REPO, RepoRules};
use crate::usage::Usage;
use crate::workspace::Workspace;
use std::collections::BTreeMap;

mod items;
mod reviews;
use items::items;
pub use reviews::*;

mod agent;
pub use agent::*;

fn lena() -> Person {
    Person::new("Lena", "LK")
}
fn jonas() -> Person {
    Person::new("Jonas", "JB")
}
fn mira() -> Person {
    Person::new("Mira", "MT")
}

#[allow(clippy::too_many_arguments)]
fn item(
    id: &str,
    kind: Kind,
    key: &str,
    title: &str,
    description: &str,
    (reason, reason_text): (ReasonKind, &str),
    time: &str,
    owner: Person,
) -> Item {
    let mine = owner == Person::me();
    Item {
        id: id.into(),
        kind,
        source: if kind == Kind::Ticket {
            Source::Jira
        } else {
            Source::GitHub
        },
        key: key.into(),
        title: title.into(),
        description: description.into(),
        reason,
        reason_text: reason_text.into(),
        time: time.into(),
        scope: if mine { Scope::Mine } else { Scope::Team },
        owner,
        status: TicketStatus::Open,
        priority: Priority::default(),
        branch: None,
        pr: None,
        agent_note: None,
        comments: Vec::new(),
        read: false,
        due: None,
        repo: None,
        labels: Vec::new(),
    }
}

/// The second repo of the sample data.
pub const DEMO_REPO_TOKENS: &str = "asakrejz/design-tokens";

/// Due date of the demo ticket BAU-150: lies before the day the demo was made.
pub const DEMO_OVERDUE: &str = "2026-11-09";

/// The initial state of the cockpit.
pub fn workspace() -> Workspace {
    let [bau142, pr87, bau138, pr85, bau150, doc7, pr12] = items();
    let [review87, review91, review85] = reviews(&pr87, &bau138, &pr85);

    // The agent has already implemented BAU-138.
    let mut run138 = AgentRun::new(&bau138.key, &bau138.title);
    run138.files = review91.files.iter().map(|f| f.name.clone()).collect();
    run138.state = RunState::Done;
    run138.completed = STEPS;
    run138.pr_number = Some(91);
    run138.repo = DEMO_REPO.into();
    let pricing = crate::Pricing::default();
    for step in 0..STEPS {
        let (input, output, millis) = step_tokens(&run138.ticket_key, step);
        run138
            .step_usage
            .push(Usage::priced(input, output, millis, &pricing));
    }

    Workspace {
        items: vec![bau142, pr87, bau138, pr85, bau150, doc7, pr12],
        reviews: vec![review87, review91, review85],
        runs: vec![run138],
        log: Vec::new(),
        // BAU-138 hangs on #91 (review requested) and is already in „Review“.
        status_seen: vec![("BAU-138".to_string(), TicketStatus::Review)],
        repo_rules: BTreeMap::from([(DEMO_REPO.to_string(), repo_rules())]),
        ..Workspace::default()
    }
}

/// The sample data plus a second repo with a pull request of the same number (`#87` in both
/// repos), so that the interface shows that equal numbers from two repos are two different
/// pull requests. This is what the server's seed `demo` and the browser demo start with.
pub fn multi_repo_workspace() -> Workspace {
    let mut ws = workspace();
    let pr87t = Item {
        scope: Scope::Team,
        repo: Some(DEMO_REPO_TOKENS.into()),
        labels: vec!["docs".into()],
        branch: Some("docs/dark-mode-contrast".into()),
        pr: Some(PrRef {
            number: 87,
            repo: DEMO_REPO_TOKENS.into(),
            draft: false,
            checks: Checks::Passed,
            changes_requested: false,
            merged: false,
            review_requested: true,
        }),
        ..item(
            "pr87-tokens",
            Kind::PullRequest,
            "#87",
            "Tokens: Kontrast im Dark Mode",
            "Hebt den Kontrast der Fokus-Tokens im Dark Mode an. Gleiche Nummer wie #87 im anderen Repo, aber ein eigener Pull Request.",
            (ReasonKind::ReviewRequested, "Review angefragt von Mira"),
            "vor 5 Std.",
            mira(),
        )
    };
    let review = Review {
        number: 87,
        repo: DEMO_REPO_TOKENS.into(),
        title: pr87t.title.clone(),
        description: pr87t.description.clone(),
        author: mira(),
        branch: pr87t.branch.clone().unwrap_or_default(),
        base: "main".into(),
        ticket_key: Some("DOC-7".into()),
        checks: Checks::Passed,
        files: vec![FileDiff::new(
            "tokens.json",
            &[
                (31, Ctx, "  \"sys-focus-ring\": {"),
                (32, Del, "    \"dark\": \"#5b8def\""),
                (32, Add, "    \"dark\": \"#7aa2ff\""),
                (33, Ctx, "  }"),
            ],
        )],
        agent_reviewed: false,
        suggestions: Vec::new(),
        line_comments: Vec::new(),
        verdict: Verdict::Comment,
        sent: false,
        check_runs: passing_checks(),
        merged: None,
    };
    ws.items.push(pr87t);
    ws.reviews.push(review);
    ws
}

/// The rules the demo repo starts with: workflows and lock files stay off limits.
pub fn repo_rules() -> RepoRules {
    RepoRules {
        forbidden_paths: vec![".github/workflows/**".into(), "*.lock".into()],
        ..RepoRules::default()
    }
}
