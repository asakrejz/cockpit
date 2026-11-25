//! The requests of outward calls as plain data (method, path, body), and the pure parts of the
//! calls that take several requests. The clients send them; nothing here touches the network.

use cockpit_core::{MergeMethod, TicketStatus};
use serde_json::json;

use super::{Request, ReviewData};
use crate::jira::dto::Transition;
use crate::jira::{status_from, text_to_adf};

/// A comment on a Jira ticket (the text goes out as ADF).
pub fn jira_comment_request(key: &str, text: &str) -> Request {
    Request {
        method: "POST",
        path: format!("/rest/api/3/issue/{key}/comment"),
        body: Some(json!({ "body": text_to_adf(text) })),
    }
}

/// A comment on a pull request (GitHub files it under the issue of the same number).
pub fn github_comment_request(repo: &str, number: u32, text: &str) -> Request {
    Request {
        method: "POST",
        path: format!("/repos/{repo}/issues/{number}/comments"),
        body: Some(json!({ "body": text })),
    }
}

pub fn review_request(repo: &str, number: u32, review: &ReviewData) -> Request {
    Request {
        method: "POST",
        path: format!("/repos/{repo}/pulls/{number}/reviews"),
        body: Some(review.body()),
    }
}

/// The merge with the commit we have seen: a new push makes GitHub refuse it.
pub fn merge_request(repo: &str, number: u32, method: MergeMethod, sha: &str) -> Request {
    let mut body = json!({ "merge_method": method.id() });
    if !sha.is_empty() {
        body["sha"] = json!(sha);
    }
    Request {
        method: "PUT",
        path: format!("/repos/{repo}/pulls/{number}/merge"),
        body: Some(body),
    }
}

/// The transition that leads to the target status: the first one in the list whose target status
/// is `target`. When several lead there, the order Jira lists them in decides; the names of the
/// transitions play no part.
pub fn choose_transition(list: &[Transition], target: TicketStatus) -> Option<&Transition> {
    list.iter().find(|t| status_from(&t.to) == target)
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
