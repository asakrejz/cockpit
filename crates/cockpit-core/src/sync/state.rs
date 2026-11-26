//! The sync state per source: problems, last success and the hints.

use cockpit_i18n::{Msg, msg};

use super::*;

/// What a fetch failed on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// 401: token missing, expired or revoked.
    Unauthorized,
    /// 403: token valid, but without permission.
    NoPermission,
    /// Quota used up; until then (milliseconds since 1970) nothing is queried.
    RateLimited { until_ms: Option<u64> },
    /// No connection to the service.
    Offline,
    /// 403 at GitHub Projects: the credential lacks this right (see [`crate::permissions`]).
    MissingRight {
        right: crate::permissions::Right,
        kind: crate::permissions::TokenKind,
    },
    /// Everything else, with a message of its own (without key). A stored plain text reads as a
    /// raw message.
    Other(#[serde(deserialize_with = "msg_or_text")] Msg),
}

/// A [`Msg`], or a plain text that becomes [`Msg::raw`].
fn msg_or_text<'de, D>(deserializer: D) -> Result<Msg, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Msg(Msg),
        Text(String),
    }
    Ok(match Stored::deserialize(deserializer)? {
        Stored::Msg(m) => m,
        Stored::Text(t) => Msg::raw(t),
    })
}

impl Problem {
    pub fn msg(&self) -> Msg {
        match self {
            Problem::Unauthorized => msg!(CoreSyncProblemUnauthorized),
            Problem::NoPermission => msg!(CoreSyncProblemNoPermission),
            Problem::RateLimited { until_ms: Some(_) } => msg!(CoreSyncProblemRateLimitedPaused),
            Problem::RateLimited { until_ms: None } => msg!(CoreSyncProblemRateLimited),
            Problem::Offline => msg!(CoreSyncProblemOffline),
            Problem::MissingRight { right, kind } => right.msg(*kind),
            Problem::Other(m) => m.clone(),
        }
    }
}

/// How the sync of a source stands. If the source is missing from the list, it is off.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SourceState {
    pub source: Source,
    /// `None` = the last fetch was successful.
    #[serde(default)]
    pub problem: Option<Problem>,
    /// Last successful sync (milliseconds since 1970).
    #[serde(default)]
    pub last_ok_ms: Option<u64>,
    /// Last attempt.
    #[serde(default)]
    pub last_try_ms: u64,
    /// Since when the current problem lasts (milliseconds since 1970); `None` while all is well.
    /// Stays while the same kind of problem repeats, so "seit 10 Min." is stable.
    #[serde(default)]
    pub since_ms: Option<u64>,
    /// Hints from the last successful fetch as plain text, the old form that is still read; new ones
    /// are in [`SourceState::warning_msgs`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Hints from the last successful fetch as code and parameters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warning_msgs: Vec<cockpit_i18n::Msg>,
}

impl SourceState {
    pub fn is_ok(&self) -> bool {
        self.problem.is_none()
    }

    /// The hints of the last successful fetch in `lang`: their messages; for a state saved
    /// earlier, which only has the plain texts, those.
    pub fn warning_lines(&self, lang: cockpit_i18n::Lang) -> Vec<String> {
        if self.warning_msgs.is_empty() {
            self.warnings.clone()
        } else {
            self.warning_msgs.iter().map(|m| m.render(lang)).collect()
        }
    }

    /// Short state statement.
    pub fn id(&self) -> &'static str {
        match &self.problem {
            None => "ok",
            Some(Problem::Unauthorized) => "unauthorized",
            Some(Problem::NoPermission | Problem::MissingRight { .. }) => "no_permission",
            Some(Problem::RateLimited { .. }) => "quota",
            Some(Problem::Offline) => "offline",
            Some(Problem::Other(_)) => "error",
        }
    }
}

/// Everything the sync records in the workspace.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncState {
    /// State per queried source.
    #[serde(default)]
    pub sources: Vec<SourceState>,
    /// IDs of the entries that come from a service. Only for them do actions go to the outside.
    #[serde(default)]
    pub items: Vec<String>,
    /// Pull requests that come from GitHub.
    #[serde(default)]
    pub prs: Vec<PrLink>,
    /// The GitHub projects of the last successful sync (fields and their choices): what is
    /// needed to write a status or another field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<crate::project::ProjectMeta>,
}

impl SyncState {
    /// The project with this node ID.
    pub fn project(&self, id: &str) -> Option<&crate::project::ProjectMeta> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn state_of(&self, source: Source) -> Option<&SourceState> {
        self.sources.iter().find(|s| s.source == source)
    }

    /// Does this entry come from a service (and not from the sample data)?
    pub fn is_synced(&self, item_id: &str) -> bool {
        self.items.iter().any(|i| i == item_id)
    }

    /// The link of a pull request; a plain number finds the first with that number.
    pub fn pr(&self, key: impl Into<PrKey>) -> Option<&PrLink> {
        let key = key.into();
        self.prs.iter().find(|p| key.matches(&p.repo, p.number))
    }

    /// Until when does the sync of this source pause because of the quota?
    pub fn paused_until(&self, source: Source, now_ms: u64) -> Option<u64> {
        match self.state_of(source)?.problem {
            Some(Problem::RateLimited { until_ms: Some(t) }) if t > now_ms => Some(t),
            _ => None,
        }
    }

    pub(crate) fn set_state(
        &mut self,
        source: Source,
        problem: Option<Problem>,
        now_ms: u64,
        warnings: &[cockpit_i18n::Msg],
    ) {
        let entry = match self.sources.iter().position(|s| s.source == source) {
            Some(i) => &mut self.sources[i],
            None => {
                self.sources.push(SourceState {
                    source,
                    problem: None,
                    last_ok_ms: None,
                    last_try_ms: now_ms,
                    since_ms: None,
                    warnings: Vec::new(),
                    warning_msgs: Vec::new(),
                });
                self.sources.last_mut().expect("just inserted")
            }
        };
        entry.last_try_ms = now_ms;
        if problem.is_none() {
            entry.last_ok_ms = Some(now_ms);
            entry.warnings.clear();
            entry.warning_msgs = warnings.iter().take(MAX_WARNINGS).cloned().collect();
        }
        entry.since_ms = match (&problem, &entry.problem) {
            (None, _) => None,
            (Some(new), Some(old))
                if std::mem::discriminant(new) == std::mem::discriminant(old) =>
            {
                entry.since_ms.or(Some(now_ms))
            }
            (Some(_), _) => Some(now_ms),
        };
        entry.problem = problem;
    }
}
