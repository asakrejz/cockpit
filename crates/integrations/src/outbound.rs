//! Actions with an effect on the outside: which [`Action`] goes to which service, and what does
//! the HTTP call look like? Everything here is pure (no network, no clock) and is tested without a server.
//!
//! * [`plan`] tells for an action what has to be done at the service ([`Outbound`]) – or `None`
//!   if the action stays local (other actions, entries from the sample data, source off).
//! * [`Outbound::request`] builds method, path and body from it, as far as one call suffices.
//!   Multi-step things (Jira transition, branch, re-run check) live in the clients; the
//!   pure parts of them (selection, bodies) live here.
//!
//! The rule „only you may merge“ is checked by `Workspace::apply` and runs **before** the call.

use cockpit_core::{
    Action, Decision, DiffSide, LineKind, MergeMethod, PrKey, Review, TicketStatus, Verdict,
    Workspace,
};
use serde_json::{Value, json};

use crate::jira::dto::Transition;
use crate::jira::{status_from, text_to_adf};

/// What the server has in terms of services.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Targets {
    pub github: bool,
    pub jira: bool,
    /// Configured GitHub repos (`owner/name`). A branch for a Jira ticket goes to the repo the
    /// routing rules name for its project, otherwise to the first one.
    pub repos: Vec<String>,
    /// Which repo a ticket belongs to (see [`cockpit_core::AgentRepos`]).
    pub routing: cockpit_core::AgentRepos,
}

/// What has to be done at the service.
#[derive(Clone, Debug, PartialEq)]
pub enum Outbound {
    JiraComment {
        key: String,
        text: String,
    },
    JiraStatus {
        key: String,
        status: TicketStatus,
    },
    GitHubComment {
        repo: String,
        number: u32,
        text: String,
    },
    GitHubBranch {
        repo: String,
        name: String,
    },
    GitHubReview {
        repo: String,
        number: u32,
        review: ReviewData,
    },
    GitHubMerge {
        repo: String,
        number: u32,
        method: MergeMethod,
        /// The commit we have seen – a new push makes the merge fail.
        sha: String,
    },
    GitHubRecheck {
        repo: String,
        number: u32,
        check: String,
    },
}

/// A review as it goes to GitHub.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewData {
    /// `COMMENT`, `APPROVE` or `REQUEST_CHANGES`
    pub verdict: &'static str,
    pub text: String,
    pub comments: Vec<LineComment>,
    /// The commit we have reviewed.
    pub sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineComment {
    pub path: String,
    pub line: u32,
    /// First line of a range of lines (on the same side). `None` = a single line.
    pub start_line: Option<u32>,
    /// `RIGHT` (added and unchanged lines) or `LEFT` (removed).
    pub page: &'static str,
    pub text: String,
}

/// A single HTTP call.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub method: &'static str,
    pub path: String,
    pub body: Option<Value>,
}

/// Why an action cannot be mapped to the outside (German message).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanError(pub String);

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Determines the call for an action. `before` is the state before, `after` the state after
/// a trial application (that is how we know e.g. the name of the new branch). `Ok(None)`:
/// stays local.
pub fn plane(
    before: &Workspace,
    after: &Workspace,
    action: &Action,
    targets: &Targets,
) -> Result<Option<Outbound>, PlanError> {
    // `(repo, head commit, number)` of a synced pull request.
    let repo_of_pr = |key: &PrKey| -> Option<(String, String, u32)> {
        before
            .sync
            .pr(key)
            .filter(|_| targets.github)
            .map(|l| (l.repo.clone(), l.head_sha.clone(), l.number))
    };
    Ok(match action {
        Action::Reply { item, text } => {
            let Some(i) = before.item(item).filter(|i| before.sync.is_synced(&i.id)) else {
                return Ok(None);
            };
            let text = text.trim().to_string();
            if text.is_empty() {
                return Ok(None);
            }
            match (i.source, i.pr.as_ref()) {
                (cockpit_core::Source::Jira, _) if targets.jira => Some(Outbound::JiraComment {
                    key: i.key.clone(),
                    text,
                }),
                (cockpit_core::Source::GitHub, Some(pr)) => repo_of_pr(&pr.key())
                    .map(|(repo, _, number)| Outbound::GitHubComment { repo, number, text }),
                _ => None,
            }
        }
        Action::SetStatus { item, status } => {
            let Some(i) = before.item(item).filter(|i| before.sync.is_synced(&i.id)) else {
                return Ok(None);
            };
            (i.source == cockpit_core::Source::Jira && i.is_ticket() && targets.jira).then(|| {
                Outbound::JiraStatus {
                    key: i.key.clone(),
                    status: *status,
                }
            })
        }
        Action::CreateBranch { item } => {
            let Some(i) = before.item(item).filter(|i| before.sync.is_synced(&i.id)) else {
                return Ok(None);
            };
            if !(i.source == cockpit_core::Source::Jira && targets.github) {
                return Ok(None);
            }
            // The repo of the ticket's project if the rules name one that is synced.
            let routed = targets
                .routing
                .choose(i.project())
                .repo()
                .and_then(|r| targets.repos.iter().find(|c| c.as_str() == r));
            let Some(repo) = routed.or_else(|| targets.repos.first()) else {
                return Err(PlanError(
                    "Für Branches ist kein GitHub-Repo konfiguriert".into(),
                ));
            };
            let name = after
                .item(item)
                .and_then(|n| n.branch.clone())
                .ok_or_else(|| PlanError("Der Branch-Name steht nicht fest".into()))?;
            Some(Outbound::GitHubBranch {
                repo: repo.clone(),
                name,
            })
        }
        Action::SendReview { review, comment } => {
            let (Some(r), Some((repo, sha, number))) = (before.review(review), repo_of_pr(review))
            else {
                return Ok(None);
            };
            Some(Outbound::GitHubReview {
                repo,
                number,
                review: review_data(r, comment, &sha),
            })
        }
        Action::MergePr { review, method } => {
            repo_of_pr(review).map(|(repo, sha, number)| Outbound::GitHubMerge {
                repo,
                number,
                method: *method,
                sha,
            })
        }
        Action::RestartCheck { review, check } => {
            repo_of_pr(review).map(|(repo, _, number)| Outbound::GitHubRecheck {
                repo,
                number,
                check: check.clone(),
            })
        }
        _ => None,
    })
}

// ---------- Review ----------

fn verdict(v: Verdict) -> &'static str {
    match v {
        Verdict::Comment => "COMMENT",
        Verdict::Approve => "APPROVE",
        Verdict::RequestChanges => "REQUEST_CHANGES",
    }
}

/// `datei · Zeile 12` → `("datei", 12)`.
fn location_read(location: &str) -> Option<(&str, u32)> {
    let (file, line) = location.rsplit_once(" · Zeile ")?;
    Some((file.trim(), line.trim().parse().ok()?))
}

/// Builds the review from the state: verdict, own text, approved suggestions of the agent
/// and all own line comments (with `suggestion` blocks). Whatever cannot be assigned to a line
/// ends up in the text of the review, so that nothing gets lost.
pub fn review_data(review: &Review, comment: &str, sha: &str) -> ReviewData {
    let mut text = comment.trim().to_string();
    let mut lines: Vec<LineComment> = Vec::new();
    let mut lose: Vec<String> = Vec::new();

    for s in review
        .suggestions
        .iter()
        .filter(|s| s.decision == Some(Decision::Approved))
    {
        // Structured position (real agent) first, the old „datei · Zeile N“ for the demo rules.
        let place = if s.path.is_empty() {
            location_read(&s.location).map(|(file, nr)| (file, nr, None, DiffSide::Right))
        } else {
            Some((s.path.as_str(), s.line, s.start_line, s.side))
        }
        .filter(|(file, nr, _, side)| review.has_line(file, *side, *nr));
        match place {
            Some((file, nr, start, side)) => lines.push(LineComment {
                path: file.into(),
                line: nr,
                // A range only goes out if its first line exists on the same side, too.
                start_line: start.filter(|&st| st < nr && review.has_line(file, side, st)),
                page: side.id(),
                text: s.body(),
            }),
            None => lose.push(format!("{}: {}", s.location, s.body())),
        }
    }
    for c in &review.line_comments {
        let line = review
            .files
            .iter()
            .find(|f| f.name == c.file)
            .and_then(|f| f.lines.get(c.line));
        match line {
            Some(l) => lines.push(LineComment {
                path: c.file.clone(),
                line: l.no,
                start_line: None,
                page: if l.kind == LineKind::Del {
                    "LEFT"
                } else {
                    "RIGHT"
                },
                text: c.body(),
            }),
            None => lose.push(format!("{}: {}", c.file, c.body())),
        }
    }
    if !lose.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&lose.join("\n\n"));
    }
    // GitHub requires text when there are no line comments (and for „Changes“).
    if text.is_empty()
        && (review.verdict == Verdict::RequestChanges
            || (review.verdict == Verdict::Comment && lines.is_empty()))
    {
        text = match review.verdict {
            Verdict::RequestChanges => "Änderungen angefordert.".into(),
            _ => "Review aus dem Cockpit.".into(),
        };
    }
    ReviewData {
        verdict: verdict(review.verdict),
        text,
        comments: lines,
        sha: sha.into(),
    }
}

// ---------- Calls ----------

impl ReviewData {
    fn body(&self) -> Value {
        let mut b = json!({ "event": self.verdict });
        if !self.text.is_empty() {
            b["body"] = json!(self.text);
        }
        if !self.sha.is_empty() {
            b["commit_id"] = json!(self.sha);
        }
        if !self.comments.is_empty() {
            b["comments"] = self
                .comments
                .iter()
                .map(|k| {
                    let mut c =
                        json!({"path": k.path, "line": k.line, "side": k.page, "body": k.text});
                    if let Some(start) = k.start_line {
                        c["start_line"] = json!(start);
                        c["start_side"] = json!(k.page);
                    }
                    c
                })
                .collect();
        }
        b
    }
}

impl Outbound {
    /// Name of the service for messages.
    pub fn service(&self) -> &'static str {
        match self {
            Outbound::JiraComment { .. } | Outbound::JiraStatus { .. } => "Jira",
            _ => "GitHub",
        }
    }

    /// The one call, as far as one suffices. Multi-step flows return `None`.
    pub fn request(&self) -> Option<Request> {
        Some(match self {
            Outbound::JiraComment { key, text } => Request {
                method: "POST",
                path: format!("/rest/api/3/issue/{key}/comment"),
                body: Some(json!({ "body": text_to_adf(text) })),
            },
            Outbound::GitHubComment { repo, number, text } => Request {
                method: "POST",
                path: format!("/repos/{repo}/issues/{number}/comments"),
                body: Some(json!({ "body": text })),
            },
            Outbound::GitHubReview {
                repo,
                number,
                review,
            } => Request {
                method: "POST",
                path: format!("/repos/{repo}/pulls/{number}/reviews"),
                body: Some(review.body()),
            },
            Outbound::GitHubMerge {
                repo,
                number,
                method,
                sha,
            } => {
                let mut b = json!({ "merge_method": method.id() });
                if !sha.is_empty() {
                    b["sha"] = json!(sha);
                }
                Request {
                    method: "PUT",
                    path: format!("/repos/{repo}/pulls/{number}/merge"),
                    body: Some(b),
                }
            }
            Outbound::JiraStatus { .. }
            | Outbound::GitHubBranch { .. }
            | Outbound::GitHubRecheck { .. } => return None,
        })
    }
}

/// The transition that leads to the target status: first one with a matching status, if there are several
/// the one whose target name comes closest to the German or English name of the status
/// (otherwise the first).
pub fn choose_transition(list: &[Transition], target: TicketStatus) -> Option<&Transition> {
    let matching: Vec<&Transition> = list
        .iter()
        .filter(|t| status_from(&t.to) == target)
        .collect();
    matching.first().copied()
}

pub fn transition_request(key: &str, id: &str) -> Request {
    Request {
        method: "POST",
        path: format!("/rest/api/3/issue/{key}/transitions"),
        body: Some(json!({ "transition": { "id": id } })),
    }
}

pub fn branch_request(repo: &str, name: &str, sha: &str) -> Request {
    Request {
        method: "POST",
        path: format!("/repos/{repo}/git/refs"),
        body: Some(json!({ "ref": format!("refs/heads/{name}"), "sha": sha })),
    }
}

/// Deletes one branch (`DELETE /repos/{repo}/git/refs/heads/{name}`). The caller checks the name
/// first (see `cockpit_core::branch::deletable`); this only builds the request.
pub fn branch_delete_request(repo: &str, name: &str) -> Request {
    Request {
        method: "DELETE",
        path: format!("/repos/{repo}/git/refs/heads/{name}"),
        body: None,
    }
}

pub fn pull_request_request(
    repo: &str,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
    draft: bool,
) -> Request {
    Request {
        method: "POST",
        path: format!("/repos/{repo}/pulls"),
        body: Some(json!({
            "title": title, "head": head, "base": base, "body": body, "draft": draft,
            "maintainer_can_modify": false,
        })),
    }
}

pub fn check_again_request(repo: &str, check_id: u64) -> Request {
    Request {
        method: "POST",
        path: format!("/repos/{repo}/check-runs/{check_id}/rerequest"),
        body: None,
    }
}

#[cfg(test)]
mod tests;
