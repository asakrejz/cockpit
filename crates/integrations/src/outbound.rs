//! Actions with an effect on the outside: which [`Action`] goes to which service, and what does
//! the HTTP call look like? Everything here is pure (no network, no clock) and is tested without a server.
//!
//! * [`plan`] tells for an action what has to be done at the service ([`Outbound`]) – or `None`
//!   if the action stays local (other actions, entries from the sample data, source off).
//! * [`Outbound::request`] builds method, path and body from it, as far as one call suffices.
//!   Multi-step things (Jira transition, branch, re-run check) live in the clients; the
//!   pure parts of them (selection, bodies) live here.
//!
//! Only an action that requires `Actor::User` (`Action::required_actor`) can become a call here.
//! `Workspace::apply` checks that and the other rules (e.g. „only you may merge“) **before** the call;
//! the test `only_actions_that_require_the_user_become_calls` walks through every action.

use cockpit_core::{
    Action, Decision, LineKind, MergeMethod, PrKey, Review, TicketStatus, Verdict, Workspace,
};
use serde_json::{Value, json};

use crate::jira::dto::Transition;
use crate::jira::{status_from, text_to_adf};

/// What the server has in terms of services.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Targets {
    pub github: bool,
    pub jira: bool,
    /// GitHub Projects are set up as a source of tickets.
    pub projects: bool,
    /// The repo a branch for a Jira ticket goes to: the default repo of the agent
    /// (`[agent] repo`, or the only configured one). `None` when there is none or there are
    /// several without a default; the plan then asks the user to set one.
    pub default_repo: Option<String>,
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
    /// GitHub Projects: set a field of an item (the status is the field of its column).
    ProjectField {
        project_id: String,
        item_id: String,
        field_id: String,
        /// `None` clears the field.
        value: Option<cockpit_core::FieldInput>,
    },
    /// GitHub Projects: title and body of a draft issue.
    DraftEdit {
        draft_id: String,
        title: String,
        body: String,
    },
    /// GitHub Projects: a change at the issue itself (comment, state, text, assignee, label).
    Issue {
        repo: String,
        number: u32,
        write: crate::github::projects::IssueWrite,
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

/// Why an action cannot be mapped to the outside. The message is what people read; `Display`
/// is its English text, for logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanError(pub cockpit_i18n::Msg);

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.render(cockpit_i18n::Lang::En))
    }
}

/// Determines the call for an action. `before` is the state before, `after` the state after
/// a trial application (that is how we know e.g. the name of the new branch). `Ok(None)`:
/// stays local.
pub fn plan_call(
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
    if let Some(planned) = plan_project(before, action, targets) {
        return planned;
    }
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
            // The default repo; without one the user has to set it (the button offers no choice).
            let Some(repo) = targets.default_repo.as_ref() else {
                return Err(PlanError(cockpit_i18n::msg!(IntegrationsPlanChooseRepo)));
            };
            let name = after
                .item(item)
                .and_then(|n| n.branch.clone())
                .ok_or_else(|| PlanError(cockpit_i18n::msg!(IntegrationsPlanNoBranchName)))?;
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

// ---------- GitHub Projects ----------

/// The call for an action on a ticket of a GitHub project. `None` = the action does not concern one
/// (the caller goes on with Jira and pull requests). A ticket from a project needs the connection
/// to GitHub Projects: without it nothing is changed locally either, because the next sync would
/// undo it.
fn plan_project(
    before: &Workspace,
    action: &Action,
    targets: &Targets,
) -> Option<Result<Option<Outbound>, PlanError>> {
    use crate::github::projects::{Assignee, IssueWrite, priority_of};
    use cockpit_core::project::option_for;
    use cockpit_core::{FieldInput, Person};
    use cockpit_i18n::msg;

    let id = match action {
        Action::Reply { item, .. }
        | Action::SetStatus { item, .. }
        | Action::EditTicket { item, .. }
        | Action::AssignTicket { item, .. }
        | Action::SetPriority { item, .. }
        | Action::SetLabel { item, .. }
        | Action::SetIssueClosed { item, .. }
        | Action::SetProjectField { item, .. }
        | Action::ConvertDraft { item, .. } => item,
        _ => return None,
    };
    let item = before
        .item(id)
        .filter(|i| i.source == cockpit_core::Source::GitHubProject)
        .filter(|i| before.sync.is_synced(&i.id))?;
    let p = item.project.as_ref()?;
    // Creating and converting have flows of their own (several calls, the answer is booked).
    if matches!(action, Action::ConvertDraft { .. }) {
        return Some(Ok(None));
    }
    let fail = |m: cockpit_i18n::Msg| Some(Err(PlanError(m)));
    if !targets.projects {
        return fail(msg!(IntegrationsPlanProjectsOff));
    }
    let issue = || -> Option<(String, u32)> { Some((item.repo.clone()?, p.number?)) };
    let field_call = |field_id: &str, value: Option<FieldInput>| Outbound::ProjectField {
        project_id: p.project_id.clone(),
        item_id: p.item_id.clone(),
        field_id: field_id.to_string(),
        value,
    };
    let issue_call = |write: IssueWrite| {
        issue().map(|(repo, number)| Outbound::Issue {
            repo,
            number,
            write,
        })
    };
    Some(Ok(match action {
        Action::Reply { text, .. } => {
            let text = text.trim().to_string();
            if text.is_empty() {
                return Some(Ok(None));
            }
            if p.draft {
                return fail(msg!(IntegrationsPlanDraftNoComments));
            }
            issue_call(IssueWrite::Comment(text))
        }
        Action::SetStatus { status, .. } => {
            let Some(meta) = before.sync.project(&p.project_id) else {
                return fail(msg!(IntegrationsPlanNoProject));
            };
            let Some(def) = meta.status_def() else {
                return fail(msg!(IntegrationsPlanNoStatusField));
            };
            let Some(option) = option_for(&def.options, *status) else {
                return fail(msg!(IntegrationsPlanNoColumn, status = status.name_msg()));
            };
            Some(field_call(
                &def.id,
                Some(FieldInput::Select(option.id.clone())),
            ))
        }
        Action::EditTicket {
            title, description, ..
        } => {
            let (title, body) = (title.trim().to_string(), description.trim().to_string());
            if p.draft {
                Some(Outbound::DraftEdit {
                    draft_id: p.content_id.clone(),
                    title,
                    body,
                })
            } else {
                issue_call(IssueWrite::Edit { title, body })
            }
        }
        Action::AssignTicket { person, .. } => {
            if p.draft {
                return fail(msg!(IntegrationsPlanDraftNoAssignee));
            }
            let who = if *person == Person::me() {
                Assignee::Me
            } else if person.nobody {
                Assignee::Nobody
            } else {
                Assignee::Login(person.name.clone())
            };
            issue_call(IssueWrite::Assign(who))
        }
        Action::SetPriority { priority, .. } => {
            let Some(meta) = before.sync.project(&p.project_id) else {
                return fail(msg!(IntegrationsPlanNoProject));
            };
            let def = meta
                .field_named("Priority")
                .filter(|d| d.kind == cockpit_core::FieldKind::SingleSelect);
            let option = def.and_then(|d| {
                d.options
                    .iter()
                    .find(|o| priority_of(&o.name) == *priority)
                    .map(|o| (d.id.clone(), o.id.clone()))
            });
            let Some((field, option)) = option else {
                return fail(msg!(IntegrationsPlanNoPriority));
            };
            Some(field_call(&field, Some(FieldInput::Select(option))))
        }
        Action::SetLabel { label, on, .. } => issue_call(IssueWrite::Label {
            label: label.trim().to_string(),
            on: *on,
        }),
        Action::SetIssueClosed { closed, .. } => issue_call(IssueWrite::State { closed: *closed }),
        Action::SetProjectField {
            field_id, value, ..
        } => Some(field_call(field_id, value.clone())),
        _ => return None,
    }))
}

// ---------- Review ----------

fn verdict(v: Verdict) -> &'static str {
    match v {
        Verdict::Comment => "COMMENT",
        Verdict::Approve => "APPROVE",
        Verdict::RequestChanges => "REQUEST_CHANGES",
    }
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
        // A draft without a position (path empty) cannot be placed on a line.
        let place = Some((s.path.as_str(), s.line, s.start_line, s.side))
            .filter(|(file, ..)| !file.is_empty())
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
            // Content for GitHub, not interface text: it is sent in English, whatever language
            // the interface speaks.
            Verdict::RequestChanges => "Changes requested.".into(),
            _ => "Review from Cockpit.".into(),
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
            | Outbound::GitHubRecheck { .. }
            | Outbound::ProjectField { .. }
            | Outbound::DraftEdit { .. }
            | Outbound::Issue { .. } => return None,
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
