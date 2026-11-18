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
        sprint: None,
        updated_ms: None,
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
            author: String::new(),
            mine: false,
            requested_me: false,
            assigned_me: false,
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
        agent_summary: String::new(),
        suggestions: Vec::new(),
        line_comments: Vec::new(),
        verdict: Verdict::Comment,
        sent: false,
        check_runs: passing_checks(),
        merged: None,
    };
    ws.items.push(pr87t);
    ws.reviews.push(review);
    add_team_pull_requests(&mut ws);
    add_sprints(&mut ws);
    ws
}

/// Pull requests of colleagues that concern nobody here: the interface lists them under
/// „Alle“ and they never raise attention. Two have no ticket key in branch or title.
fn add_team_pull_requests(ws: &mut Workspace) {
    let team = [
        (
            101,
            DEMO_REPO,
            "Avatare zwischenspeichern",
            "perf/avatar-cache",
            lena(),
            "performance",
        ),
        (
            102,
            DEMO_REPO_TOKENS,
            "Tippfehler in der README",
            "docs/readme-typo",
            jonas(),
            "docs",
        ),
        (
            103,
            DEMO_REPO,
            "SHOP-5: Warenkorb-Symbol",
            "feature/shop-5-cart-icon",
            mira(),
            "design",
        ),
    ];
    for (number, repo, title, branch, author, label) in team {
        let mut pr = item(
            &format!("pr{number}"),
            Kind::PullRequest,
            &format!("#{number}"),
            title,
            "Pull Request einer Kollegin oder eines Kollegen – nur unter „Alle“ sichtbar.",
            (
                ReasonKind::Comment,
                &format!("Pull Request von {}", author.name),
            ),
            "vor 1 Tg.",
            author.clone(),
        );
        pr.repo = Some(repo.into());
        pr.branch = Some(branch.into());
        pr.labels = vec![label.into()];
        pr.status = TicketStatus::Review;
        pr.pr = Some(PrRef {
            number,
            repo: repo.into(),
            draft: false,
            checks: Checks::Passed,
            changes_requested: false,
            merged: false,
            review_requested: false,
            author: author.name.to_lowercase(),
            mine: false,
            requested_me: false,
            assigned_me: false,
        });
        ws.reviews.push(Review {
            number,
            repo: repo.into(),
            title: title.into(),
            description: pr.description.clone(),
            author,
            branch: branch.into(),
            base: "main".into(),
            ticket_key: None,
            checks: Checks::Passed,
            files: vec![FileDiff::new(
                "README.md",
                &[(1, Del, "# Projekt"), (1, Add, "# Projekt – Überblick")],
            )],
            agent_reviewed: false,
            agent_summary: String::new(),
            suggestions: Vec::new(),
            line_comments: Vec::new(),
            verdict: Verdict::Comment,
            sent: false,
            check_runs: passing_checks(),
            merged: None,
        });
        ws.items.push(pr);
    }
}

/// Sprints for the tickets, and tickets of colleagues in other sprints or none: the sprint
/// filter of the inbox has something to choose from.
fn add_sprints(ws: &mut Workspace) {
    let sprint = |name: &str, state, start: &str, end: &str| Sprint {
        name: name.into(),
        state,
        start: start.into(),
        end: end.into(),
    };
    let active = sprint(
        "Sprint 12",
        SprintState::Active,
        "2026-11-03T08:00:00.000Z",
        "2026-11-17T16:00:00.000Z",
    );
    let updated = crate::attention::day_start_ms("2026-11-10");
    for item in ws.items.iter_mut().filter(|i| i.kind == Kind::Ticket) {
        item.updated_ms = updated;
        if item.key != "DOC-7" {
            item.sprint = Some(active.clone());
        }
    }
    let ticket = |key: &str, title: &str, sprint: Option<Sprint>, days_ago: u64| {
        let mut t = item(
            &key.to_lowercase().replace('-', ""),
            Kind::Ticket,
            key,
            title,
            "Ticket einer Kollegin oder eines Kollegen – nur unter „Alle“ sichtbar.",
            (ReasonKind::Comment, "Im Sprint"),
            "vor 2 Tg.",
            jonas(),
        );
        t.sprint = sprint;
        t.updated_ms = updated.map(|u| u - days_ago * 24 * 3600 * 1000);
        t
    };
    ws.items.push(ticket(
        "WEB-21",
        "Kopfzeile auf schmalen Bildschirmen",
        Some(active.clone()),
        1,
    ));
    ws.items.push(ticket(
        "WEB-22",
        "Cookie-Hinweis überarbeiten",
        Some(sprint(
            "Sprint 13",
            SprintState::Future,
            "2026-11-17T16:00:00.000Z",
            "2026-12-01T16:00:00.000Z",
        )),
        4,
    ));
    ws.items.push(ticket(
        "WEB-23",
        "Idee: Dark-Mode-Schalter im Footer",
        None,
        40,
    ));
    ws.items.push(ticket(
        "WEB-18",
        "Bildformate prüfen (Sprint beendet)",
        Some(sprint(
            "Sprint 11",
            SprintState::Closed,
            "2026-10-20T08:00:00.000Z",
            "2026-11-03T16:00:00.000Z",
        )),
        9,
    ));
}

/// The rules the demo repo starts with: workflows and lock files stay off limits.
pub fn repo_rules() -> RepoRules {
    RepoRules {
        forbidden_paths: vec![".github/workflows/**".into(), "*.lock".into()],
        ..RepoRules::default()
    }
}
