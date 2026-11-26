//! GitHub responses → types from `cockpit-core`. Pure functions, without network.

use cockpit_core::{
    Checks, Comment, FileDiff, Item, Kind, Person, PrRef, ReasonKind, Review, Scope, Source,
    TicketStatus, Verdict,
};
use cockpit_i18n::{Msg, msg};

use super::Relation;
use super::dto::{
    CheckRun, CombinedStatus, IssueComment, Pull, PullFile, PullReview, ReviewComment,
};
use super::patch::parse_patch;
use crate::{text, time};

/// Condenses the check runs of a commit into one status.
///
/// * a run failed (`failure`, `timed_out`, `cancelled`, `action_required`,
///   `startup_failure`) → `Failed`, even if others are still running
/// * otherwise a run not completed → `Running`
/// * otherwise (also without any run) → `Passed`
pub fn aggregate_checks(runs: &[CheckRun]) -> Checks {
    let failed = |r: &CheckRun| {
        matches!(
            r.conclusion.as_deref(),
            Some("failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure")
        )
    };
    if runs.iter().any(failed) {
        Checks::Failed
    } else if runs.iter().any(|r| r.status != "completed") {
        Checks::Running
    } else {
        Checks::Passed
    }
}

/// The combined commit status as the checks of a commit (fallback when the check runs are not
/// readable). `success` → `Passed`, `failure`/`error` → `Failed`, `pending` → `Running`. Without
/// any status GitHub also says `pending`, but nothing runs then: that is `Unknown`.
pub fn status_to_checks(status: &CombinedStatus) -> Checks {
    if status.total_count == 0 {
        return Checks::Unknown;
    }
    match status.state.as_str() {
        "success" => Checks::Passed,
        "failure" | "error" => Checks::Failed,
        "pending" => Checks::Running,
        _ => Checks::Unknown,
    }
}

/// How a pull request relates to the user, from what the pull request itself says (no
/// extra call per pull request). Mentions are not part of it: GitHub lists them only in the
/// comments, which are not loaded for pull requests of colleagues.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrRelation {
    /// I am the author.
    pub author: bool,
    /// A review from me (as a person) is requested.
    pub requested: bool,
    /// I am an assignee.
    pub assigned: bool,
}

impl PrRelation {
    /// `me` is the GitHub login of the user (without regard to case).
    pub fn of(pull: &Pull, me: &str) -> Self {
        let is_me = |login: &str| !me.is_empty() && login.eq_ignore_ascii_case(me);
        Self {
            author: is_me(&pull.user.login),
            requested: pull.requested_reviewers.iter().any(|u| is_me(&u.login)),
            assigned: pull.assignees.iter().any(|u| is_me(&u.login)),
        }
    }

    /// Does the pull request concern me?
    pub fn mine(&self) -> bool {
        self.author || self.requested || self.assigned
    }
}

impl From<Relation> for PrRelation {
    fn from(r: Relation) -> Self {
        Self {
            author: r == Relation::Own,
            requested: r == Relation::ReviewRequested,
            assigned: false,
        }
    }
}

/// A pull request as an inbox entry, for the relation of the search it came from.
pub fn pull_to_item(repo: &str, pull: &Pull, checks: Checks, relation: Relation) -> Item {
    pull_to_item_for(repo, pull, checks, relation.into())
}

/// A pull request as an inbox entry.
///
/// `repo` is `owner/name` and goes into the ID, so that equal numbers from different
/// repos do not overlap. A requested review gives
/// `ReviewRequested`; an own pull request `ChecksFailed` or `Comment`; assigned to me only and
/// pull requests of colleagues give `Comment` – the latter with the scope `Team`.
/// Comments are empty – they come through [`comments_to_core`].
pub fn pull_to_item_for(repo: &str, pull: &Pull, checks: Checks, relation: PrRelation) -> Item {
    let (reason, reason_msg) = if relation.requested {
        (ReasonKind::ReviewRequested, msg!(ReasonReviewRequested))
    } else if relation.author {
        match checks {
            Checks::Failed => (ReasonKind::ChecksFailed, msg!(ReasonChecksFailed)),
            _ => (ReasonKind::Comment, msg!(ReasonOwnPr)),
        }
    } else if relation.assigned {
        (ReasonKind::Comment, msg!(ReasonAssigned))
    } else {
        (
            ReasonKind::Comment,
            msg!(IntegrationsSyncPrBy, author = &pull.user.login),
        )
    };
    Item {
        id: text::id_from(&["gh", repo, &pull.number.to_string()]),
        kind: Kind::PullRequest,
        source: Source::GitHub,
        key: format!("#{}", pull.number),
        title: pull.title.clone(),
        description: pull.body.clone().unwrap_or_default(),
        description_rich: None,
        reason,
        reason_msg: Some(reason_msg),
        reason_text: String::new(),
        time: String::new(),
        scope: if relation.mine() {
            Scope::Mine
        } else {
            Scope::Team
        },
        owner: text::person(&pull.user.login),
        status: TicketStatus::Review,
        priority: cockpit_core::Priority::default(),
        branch: Some(pull.head.name.clone()),
        pr: Some(PrRef {
            number: pull.number,
            repo: repo.to_string(),
            draft: pull.draft,
            checks,
            changes_requested: false,
            merged: false,
            review_requested: relation.requested,
            author: pull.user.login.clone(),
            mine: relation.mine(),
            requested_me: relation.requested,
            assigned_me: relation.assigned,
        }),
        agent_note: None,
        agent_note_msg: None,
        comments: Vec::new(),
        read: false,
        due: None,
        repo: Some(repo.to_string()),
        labels: pull.labels.iter().map(|l| l.name.clone()).collect(),
        sprint: None,
        issue_type: None,
        reporter: None,
        watching: false,
        team: None,
        updated_ms: time::parse_iso(&pull.updated_at).map(|s| s as u64 * 1000),
        url: cockpit_core::adf::safe_url(&pull.html_url).map(str::to_string),
        details: None,
        project: None,
        co_assignees: Vec::new(),
        milestone: None,
    }
}

/// Patch of a file → `FileDiff`. Without a patch (binary file, too large) the lines stay empty.
pub fn file_to_diff(file: &PullFile) -> FileDiff {
    FileDiff {
        name: file.filename.clone(),
        lines: file.patch.as_deref().map(parse_patch).unwrap_or_default(),
    }
}

/// A pull request with files as a review. The verdict starts as „Comment“, there are
/// no suggestions yet. The ticket comes from branch name or title (`BAU-142`).
pub fn pull_to_review(pull: &Pull, files: &[PullFile], checks: Checks) -> Review {
    let ticket_key = text::ticket_key(&pull.head.name).or_else(|| text::ticket_key(&pull.title));
    Review {
        number: pull.number,
        // The caller knows the repo (a pull request does not name its own).
        repo: String::new(),
        title: pull.title.clone(),
        description: pull.body.clone().unwrap_or_default(),
        author: text::person(&pull.user.login),
        branch: pull.head.name.clone(),
        base: pull.base.name.clone(),
        ticket_key,
        checks,
        files: files.iter().map(file_to_diff).collect(),
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

/// Comments of the conversation and on diff lines, sorted by time. Comments on lines
/// start with `file · line n`. `me` is the GitHub login of the user.
pub fn comments_to_core(
    issue: &[IssueComment],
    review: &[ReviewComment],
    me: &str,
) -> Vec<Comment> {
    comments_with_reviews(issue, review, &[], me)
}

/// `@login` of the user → `@Du`.
pub fn mention_for(text: &str, me: &str) -> String {
    if me.is_empty() {
        return text.to_string();
    }
    let lower = text.to_lowercase();
    let needle = format!("@{}", me.to_lowercase());
    let mut out = String::new();
    let mut copied_to = 0;
    for (pos, _) in lower.match_indices(&needle) {
        let end = pos + needle.len();
        let after = lower[end..].chars().next();
        // Whole names only: `@octocat-bot` is not `@octocat`.
        if pos < copied_to || after.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            continue;
        }
        if !text.is_char_boundary(pos) || !text.is_char_boundary(end) {
            continue;
        }
        out.push_str(&text[copied_to..pos]);
        out.push_str(&format!("@{}", cockpit_core::ME_MARK));
        copied_to = end;
    }
    out.push_str(&text[copied_to..]);
    out
}

/// Like [`comments_to_core`], plus the texts of the reviews (with the verdict in the first line).
pub fn comments_with_reviews(
    issue: &[IssueComment],
    review: &[ReviewComment],
    reviews: &[PullReview],
    me: &str,
) -> Vec<Comment> {
    let mut all: Vec<(i64, Comment)> = Vec::new();
    let mut add = |login: &str, time_iso: &str, body: String, verdict: Option<Msg>| {
        let own = login.eq_ignore_ascii_case(me);
        let author = if own {
            Person::me()
        } else {
            text::person(login)
        };
        let sorting = time::parse_iso(time_iso).unwrap_or(0);
        let text = if own { body } else { mention_for(&body, me) };
        all.push((
            sorting,
            Comment {
                author,
                time: String::new(),
                created_ms: time::parse_iso(time_iso).map(|s| s.max(0) as u64 * 1000),
                // A review shows its verdict in front of the words; the words stay the text.
                text_msg: verdict.map(|verdict| {
                    cockpit_i18n::msg!(
                        IntegrationsSyncReviewComment,
                        verdict = verdict,
                        text = text.clone()
                    )
                }),
                text,
                rich: None,
                own,
            },
        ));
    };
    for c in issue {
        add(
            &c.user.login,
            &c.created_at,
            c.body.clone().unwrap_or_default(),
            None,
        );
    }
    for c in review {
        let location = match c.line {
            Some(n) => format!("{} · Zeile {n}", c.path),
            None => c.path.clone(),
        };
        add(
            &c.user.login,
            &c.created_at,
            format!("{location}\n{}", c.body.as_deref().unwrap_or_default()),
            None,
        );
    }
    for r in reviews {
        let text = r.body.as_deref().unwrap_or_default().trim();
        let verdict = match r.state.as_str() {
            "APPROVED" => cockpit_i18n::msg!(IntegrationsSyncReviewApproved),
            "CHANGES_REQUESTED" => cockpit_i18n::msg!(IntegrationsSyncReviewChangesRequested),
            // Pure comments are already among the comments; without text there is nothing to show.
            "COMMENTED" | "DISMISSED" if !text.is_empty() => {
                cockpit_i18n::msg!(IntegrationsSyncReviewPlain)
            }
            _ => continue,
        };
        add(
            &r.user.login,
            &r.submitted_at,
            text.to_string(),
            Some(verdict),
        );
    }
    all.sort_by_key(|(t, _)| *t);
    all.into_iter().map(|(_, c)| c).collect()
}

/// Has someone other than me requested changes and not approved afterwards?
/// Decisive is each person's latest verdict (comments and dismissed ones do not count).
pub fn changes_requested(reviews: &[PullReview], me: &str) -> bool {
    let mut last: Vec<(&str, bool)> = Vec::new();
    let mut sorted: Vec<&PullReview> = reviews.iter().collect();
    sorted.sort_by_key(|r| time::parse_iso(&r.submitted_at).unwrap_or(0));
    for r in sorted {
        if r.user.login.eq_ignore_ascii_case(me) {
            continue;
        }
        let verdict = match r.state.as_str() {
            "CHANGES_REQUESTED" => true,
            "APPROVED" | "DISMISSED" => false,
            _ => continue,
        };
        match last
            .iter_mut()
            .find(|(l, _)| l.eq_ignore_ascii_case(&r.user.login))
        {
            Some(e) => e.1 = verdict,
            None => last.push((&r.user.login, verdict)),
        }
    }
    last.iter().any(|(_, requested)| *requested)
}

/// The individual check runs as the review's checks (with duration and output).
pub fn check_runs_to_core(runs: &[CheckRun]) -> Vec<cockpit_core::CheckRun> {
    runs.iter()
        .map(|r| {
            let status = aggregate_checks(std::slice::from_ref(r));
            let duration = match (
                r.started_at.as_deref().and_then(time::parse_iso),
                r.completed_at.as_deref().and_then(time::parse_iso),
            ) {
                (Some(a), Some(b)) if b >= a => (b - a) as u32,
                _ => 0,
            };
            cockpit_core::CheckRun::new(&r.name, status, duration, &r.log())
        })
        .collect()
}

#[cfg(test)]
mod status_tests {
    use super::*;

    fn status(state: &str, total_count: u32) -> CombinedStatus {
        CombinedStatus {
            state: state.into(),
            total_count,
        }
    }

    #[test]
    fn combined_status_maps_to_checks() {
        assert_eq!(status_to_checks(&status("success", 2)), Checks::Passed);
        assert_eq!(status_to_checks(&status("failure", 2)), Checks::Failed);
        assert_eq!(status_to_checks(&status("error", 1)), Checks::Failed);
        assert_eq!(status_to_checks(&status("pending", 3)), Checks::Running);
        assert_eq!(status_to_checks(&status("weird", 1)), Checks::Unknown);
    }

    #[test]
    fn no_status_at_all_is_unknown_not_running() {
        assert_eq!(status_to_checks(&status("pending", 0)), Checks::Unknown);
        assert_eq!(status_to_checks(&status("success", 0)), Checks::Unknown);
    }
}
