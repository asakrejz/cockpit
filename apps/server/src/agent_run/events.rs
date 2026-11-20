//! Reading the runner's output lines and turning them into reports.

use super::*;

/// What a container does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Only read and present a plan.
    Plan,
    /// Implement the ticket (after approval) and commit.
    Implement,
    /// Read a pull request and report review comments. Read-only like the plan, never pushes.
    Review,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Mode::Plan => "plan",
            Mode::Implement => "implement",
            Mode::Review => "review",
        }
    }
}

/// An event of the runner (a JSON line on stdout). See `apps/agent/README.md`.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Event {
    Plan {
        summary: String,
        #[serde(default)]
        files: Vec<String>,
        #[serde(default)]
        steps: Vec<String>,
    },
    Step {
        title: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        tokens_in: u64,
        #[serde(default)]
        tokens_out: u64,
        #[serde(default)]
        ms: u64,
    },
    Files {
        files: Vec<String>,
    },
    Question {
        text: String,
        #[serde(default)]
        suggestions: Vec<String>,
    },
    HintsArrived,
    /// A draft review comment; the backend checks it against the diff before it is kept.
    ReviewComment {
        path: String,
        line: u32,
        #[serde(default)]
        start_line: Option<u32>,
        side: String,
        text: String,
        #[serde(default)]
        suggestion: Option<String>,
    },
    ReviewSummary {
        text: String,
    },
    Result {
        text: String,
        branch: String,
        commit: String,
        /// A run in several repos: the branch and commit per repo that has changes. Repos the
        /// agent left alone are missing. The backend checks everything itself and only compares
        /// the branch names.
        #[serde(default)]
        repos: Vec<ResultRepo>,
    },
    Check {
        reachable: bool,
        key_ok: bool,
        #[serde(default)]
        hint: String,
    },
    Error {
        text: String,
    },
}

/// Why a line was not accepted.
#[derive(Debug, PartialEq, Eq)]
pub enum Discarded {
    TooLarge,
    Unreadable,
}

pub(super) fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut t: String = text.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

pub(super) fn list(v: Vec<String>) -> Vec<String> {
    v.into_iter()
        .take(MAX_LIST)
        .map(|s| truncate(&s, 300))
        .collect()
}

/// One repo in the result of a run in several repos.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResultRepo {
    pub repo: String,
    pub branch: String,
    #[serde(default)]
    pub commit: String,
}

/// Reads a line of the runner. Too large, unreadable or unknown lines are discarded;
/// texts, lists and numbers are limited afterwards. The backend does not trust the container.
pub fn line_read(line: &str) -> Result<Event, Discarded> {
    if line.len() > MAX_LINE {
        return Err(Discarded::TooLarge);
    }
    let e: Event = serde_json::from_str(line.trim()).map_err(|_| Discarded::Unreadable)?;
    Ok(match e {
        Event::Plan {
            summary,
            files,
            steps,
        } => Event::Plan {
            summary: truncate(&summary, MAX_TEXT),
            files: list(files),
            steps: list(steps),
        },
        Event::Step {
            title,
            text,
            tokens_in,
            tokens_out,
            ms,
        } => Event::Step {
            title: truncate(&title, 120),
            text: truncate(&text, 300),
            tokens_in: tokens_in.min(MAX_TOKEN),
            tokens_out: tokens_out.min(MAX_TOKEN),
            ms: ms.min(MAX_MS),
        },
        Event::Files { files } => Event::Files { files: list(files) },
        Event::ReviewComment {
            path,
            line,
            start_line,
            side,
            text,
            suggestion,
        } => Event::ReviewComment {
            path: truncate(&path, 300),
            line: line.min(MAX_LINE_NO),
            start_line: start_line.map(|s| s.min(MAX_LINE_NO)),
            side: truncate(&side, 10),
            text: truncate(&text, MAX_TEXT),
            suggestion: suggestion.map(|s| truncate(&s, MAX_TEXT)),
        },
        Event::ReviewSummary { text } => Event::ReviewSummary {
            text: truncate(&text, 4_000),
        },
        Event::Question { text, suggestions } => Event::Question {
            text: truncate(&text, MAX_TEXT),
            suggestions: list(suggestions).into_iter().take(6).collect(),
        },
        Event::Result {
            text,
            branch,
            commit,
            repos,
        } => Event::Result {
            text: truncate(&text, 1_500),
            branch: truncate(&branch, 200),
            commit: truncate(&commit, 64),
            repos: repos
                .into_iter()
                .take(cockpit_core::MAX_REPOS_PER_RUN)
                .map(|r| ResultRepo {
                    repo: truncate(&r.repo, 200),
                    branch: truncate(&r.branch, 200),
                    commit: truncate(&r.commit, 64),
                })
                .collect(),
        },
        Event::Check {
            reachable,
            key_ok,
            hint,
        } => Event::Check {
            reachable,
            key_ok,
            hint: truncate(&hint, 300),
        },
        Event::Error { text } => Event::Error {
            text: truncate(&text, MAX_TEXT),
        },
        same @ Event::HintsArrived => same,
    })
}

/// What an event changes in the run. `None` for events that do not (or not in this mode)
/// belong in the run: result and error are evaluated by the operation itself.
pub fn as_message(e: &Event, mode: Mode) -> Option<AgentReport> {
    match (e, mode) {
        (
            Event::Plan {
                summary,
                files,
                steps,
            },
            Mode::Plan,
        ) => Some(AgentReport::Plan {
            summary: summary.clone(),
            files: files.clone(),
            steps: steps.clone(),
        }),
        (
            Event::Step {
                title,
                text,
                tokens_in,
                tokens_out,
                ms,
            },
            Mode::Plan | Mode::Implement | Mode::Review,
        ) => Some(AgentReport::Step {
            title: title.clone(),
            text: text.clone(),
            input_tokens: *tokens_in,
            output_tokens: *tokens_out,
            millis: *ms,
        }),
        (Event::Files { files }, Mode::Implement) => Some(AgentReport::Files {
            files: files.clone(),
        }),
        (Event::Question { text, suggestions }, Mode::Implement) => Some(AgentReport::Question {
            text: text.clone(),
            suggestions: suggestions.clone(),
        }),
        (Event::HintsArrived, Mode::Implement) => Some(AgentReport::HintsDelivered { count: 1 }),
        (
            Event::ReviewComment {
                path,
                line,
                start_line,
                side,
                text,
                suggestion,
            },
            Mode::Review,
        ) => Some(AgentReport::ReviewComment {
            path: path.clone(),
            line: *line,
            start_line: *start_line,
            side: DiffSide::from_id(side)?,
            text: text.clone(),
            suggestion: suggestion.clone(),
        }),
        (Event::ReviewSummary { text }, Mode::Review) => {
            Some(AgentReport::ReviewSummary { text: text.clone() })
        }
        _ => None,
    }
}
