use super::*;

/// How a pull request gets into the target branch.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergeMethod {
    #[default]
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("merge", "Merge"),
        ("squash", "Squash"),
        ("rebase", "Rebase"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            MergeMethod::Merge => "merge",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
        }
    }

    pub fn label(self) -> &'static str {
        Self::OPTIONS
            .iter()
            .find(|(id, _)| *id == self.id())
            .map_or("", |(_, label)| label)
    }

    pub fn from_id(id: &str) -> Option<MergeMethod> {
        [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase]
            .into_iter()
            .find(|m| m.id() == id)
    }
}

/// A reason why merging is not allowed yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeBlocker {
    AlreadyMerged,
    Draft,
    ChecksFailed,
    ChecksRunning,
    ReviewNotSent,
    ChangesRequested,
}

impl MergeBlocker {
    /// Justification for the UI.
    pub fn reason_msg(self) -> cockpit_i18n::Msg {
        use cockpit_i18n::msg;
        match self {
            MergeBlocker::AlreadyMerged => msg!(CoreReviewBlockerAlreadyMerged),
            MergeBlocker::Draft => msg!(CoreReviewBlockerDraft),
            MergeBlocker::ChecksFailed => msg!(CoreReviewBlockerChecksFailed),
            MergeBlocker::ChecksRunning => msg!(CoreReviewBlockerChecksRunning),
            MergeBlocker::ReviewNotSent => msg!(CoreReviewBlockerNotSent),
            MergeBlocker::ChangesRequested => msg!(CoreReviewBlockerChanges),
        }
    }
}

/// The conditions for merging as a pure function: checks green, review sent and
/// not „Changes“. Empty = allowed. If the pull request is merged, that is the only
/// Reason.
pub fn merge_blockers(
    checks: Checks,
    review_sent: bool,
    verdict: Verdict,
    merged: bool,
) -> Vec<MergeBlocker> {
    if merged {
        return vec![MergeBlocker::AlreadyMerged];
    }
    let mut out = Vec::new();
    match checks {
        Checks::Failed => out.push(MergeBlocker::ChecksFailed),
        Checks::Running => out.push(MergeBlocker::ChecksRunning),
        // Unreadable checks do not block: GitHub itself decides at the merge.
        Checks::Passed | Checks::Unknown => {}
    }
    if !review_sent {
        out.push(MergeBlocker::ReviewNotSent);
    }
    if verdict == Verdict::RequestChanges {
        out.push(MergeBlocker::ChangesRequested);
    }
    out
}
