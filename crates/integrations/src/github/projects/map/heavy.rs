//! The part of an issue that costs a call of its own (comments, linked pull requests, issue type,
//! parent and sub-issues): read from the details, kept from one sync to the next, and put on an
//! item. Pure functions.

use cockpit_core::project::issue_key;
use cockpit_core::{Comment, DetailField, DetailValue, IssueLink, Item, PrKey};

use super::super::dto::{DetailNode, IssueNode, PrNode};
use super::{MapCtx, person_of};
use crate::time;

fn pr_key(n: &PrNode) -> Option<PrKey> {
    let (number, repo) = (n.number?, n.repository.as_ref()?);
    (!repo.name_with_owner.is_empty()).then(|| PrKey::new(&repo.name_with_owner, number))
}

fn issue_link(n: &IssueNode) -> Option<IssueLink> {
    let repo = n.repository.as_ref()?.name_with_owner.clone();
    (!repo.is_empty() && n.number > 0).then(|| IssueLink {
        key: issue_key(&repo, n.number),
        title: n.title.clone(),
        done: n.state == "CLOSED",
    })
}

/// The part of an issue that costs a call of its own: comments, linked pull requests, issue
/// type, parent and sub-issues. Kept from one sync to the next while the issue does not change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heavy {
    pub comments: Vec<Comment>,
    pub linked_prs: Vec<PrKey>,
    pub issue_type: Option<String>,
    pub parent: Option<IssueLink>,
    pub sub_issues: Vec<IssueLink>,
}

impl Heavy {
    /// What an item already carries of it.
    pub fn of_item(item: &Item) -> Option<Heavy> {
        let p = item.project.as_ref()?;
        Some(Heavy {
            comments: item.comments.clone(),
            linked_prs: p.linked_prs.clone(),
            issue_type: item.issue_type.clone(),
            parent: p.parent.clone(),
            sub_issues: p.sub_issues.clone(),
        })
    }
}

/// Reads the details of an issue.
pub fn heavy_of(d: &DetailNode, ctx: &MapCtx<'_>) -> Heavy {
    let comments = d
        .comments
        .iter()
        .flat_map(|c| c.items())
        .map(|c| {
            let login = c
                .author
                .as_ref()
                .map(|a| a.login.as_str())
                .unwrap_or("ghost");
            let own = !ctx.me.is_empty() && login.eq_ignore_ascii_case(ctx.me);
            Comment {
                author: person_of(login, ctx.me),
                time: String::new(),
                created_ms: time::parse_iso(&c.created_at).map(|s| s.max(0) as u64 * 1000),
                text: c.body.clone(),
                text_msg: None,
                rich: None,
                own,
            }
        })
        .collect();

    let mut prs: Vec<PrKey> = Vec::new();
    let mut add = |k: Option<PrKey>| {
        if let Some(k) = k
            && !prs.iter().any(|p| p.same(&k))
        {
            prs.push(k);
        }
    };
    for t in d.timeline_items.iter().flat_map(|c| c.items()) {
        match t.typename.as_str() {
            "ConnectedEvent" => add(t.subject.as_ref().and_then(pr_key)),
            "CrossReferencedEvent" => add(t.source.as_ref().and_then(pr_key)),
            _ => {}
        }
    }
    for p in d
        .closed_by_pull_requests_references
        .iter()
        .flat_map(|c| c.items())
    {
        add(pr_key(p));
    }
    Heavy {
        comments,
        linked_prs: prs,
        issue_type: d
            .issue_type
            .as_ref()
            .map(|t| t.name.clone())
            .filter(|n| !n.is_empty()),
        parent: d.parent.as_ref().and_then(issue_link),
        sub_issues: d
            .sub_issues
            .iter()
            .flat_map(|c| c.items())
            .filter_map(issue_link)
            .collect(),
    }
}

/// Puts the heavy part on an item (and the matching rows into its details).
pub fn apply_heavy(item: &mut Item, h: Heavy) {
    item.comments = h.comments;
    item.issue_type = h.issue_type;
    if let Some(details) = item.details.as_mut() {
        if let Some(t) = &item.issue_type {
            details.fields.push(DetailField {
                id: "issuetype".into(),
                name: String::new(),
                value: DetailValue::Text(t.clone()),
            });
        }
        if let Some(p) = &h.parent {
            details.fields.push(DetailField {
                id: "parent".into(),
                name: String::new(),
                value: DetailValue::Text(format!("{} {}", p.key, p.title).trim().to_string()),
            });
        }
        if !h.sub_issues.is_empty() {
            details.fields.push(DetailField {
                id: "ghp:sub_issues".into(),
                name: String::new(),
                value: DetailValue::List(
                    h.sub_issues
                        .iter()
                        .map(|s| {
                            format!("{} {} {}", if s.done { "✓" } else { "○" }, s.key, s.title)
                        })
                        .collect(),
                ),
            });
        }
    }
    if let Some(p) = item.project.as_mut() {
        p.linked_prs = h.linked_prs;
        p.parent = h.parent;
        p.sub_issues = h.sub_issues;
    }
}

/// Reads the details of an issue and puts them on the item.
pub fn apply_details(item: &mut Item, d: &DetailNode, ctx: &MapCtx<'_>) {
    apply_heavy(item, heavy_of(d, ctx));
}
