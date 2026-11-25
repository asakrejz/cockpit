//! Plain-language states for the interface: what is wrong with a source, how long it
//! lasts and what to do next. One place for the wording, so pages do not invent their own.
//!
//! Pure functions on the data the workspace already carries ([`SyncState`]); the browser only
//! renders the result. No message contains a key: problems come from [`Problem`], which is
//! free of secrets by construction.

use cockpit_i18n::{Msg, msg};

use crate::model::Source;
use crate::sync::{Problem, SourceState, SyncState};

/// How loud a notice is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeTone {
    /// Informational, nothing is lost (e.g. paused until the quota is back).
    Info,
    /// Something is wrong, last data stays visible.
    Warning,
    /// Needs the user (token rejected).
    Error,
}

/// What the user can do about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    /// Go to the setup page and renew the token.
    OpenSetup,
    /// Try again now ("Sync now").
    RetryNow,
    /// Nothing to do, the sync resumes by itself.
    Wait,
}

/// A banner for one source that is not healthy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceNotice {
    pub source: Source,
    pub tone: NoticeTone,
    /// Short headline, e.g. "GitHub not reachable".
    pub title: Msg,
    /// Cause, how long the data is old and what to do.
    pub detail: Msg,
    pub action: NoticeAction,
    /// Since when the problem lasts.
    pub since_ms: Option<u64>,
    /// When the sync tries again (rate limit only).
    pub retry_at_ms: Option<u64>,
}

/// How long ago, as a message: "just now", "5 minutes ago" ...
pub fn ago_msg(then_ms: u64, now_ms: u64) -> Msg {
    let s = now_ms.saturating_sub(then_ms) / 1000;
    match s {
        0..=59 => msg!(TimeJustNow),
        60..=3599 => msg!(TimeAgoMinutes, n = s / 60),
        3600..=86_399 => msg!(TimeAgoHours, n = s / 3600),
        _ => msg!(TimeAgoDays, n = s / 86_400),
    }
}

/// "in 20 sec.", "in 5 min.", "in 2 h": the wait until `until_ms`, as a message.
pub fn wait_msg(until_ms: u64, now_ms: u64) -> Msg {
    let s = until_ms.saturating_sub(now_ms).div_ceil(1000);
    match s {
        0 => msg!(CoreNoticeWaitNow),
        1..=59 => msg!(CoreNoticeWaitSeconds, n = s),
        60..=3599 => msg!(CoreNoticeWaitMinutes, n = s.div_ceil(60)),
        _ => msg!(CoreNoticeWaitHours, n = s.div_ceil(3600)),
    }
}

fn name(source: Source) -> &'static str {
    match source {
        Source::Jira => "Jira",
        Source::GitHub => "GitHub",
        Source::GitHubProject => "GitHub Projects",
    }
}

fn last_state(st: &SourceState, now_ms: u64) -> Msg {
    match st.last_ok_ms {
        Some(t) => msg!(CoreNoticeLastStateAt, when = ago_msg(t, now_ms)),
        None => msg!(CoreNoticeLastStateNone),
    }
}

/// The notice for one source, `None` while it is healthy.
pub fn source_notice(st: &SourceState, now_ms: u64) -> Option<SourceNotice> {
    let problem = st.problem.as_ref()?;
    let source = name(st.source);
    let last_state = last_state(st, now_ms);
    let (tone, title, detail, action, retry_at_ms) = match problem {
        Problem::Unauthorized => (
            NoticeTone::Error,
            msg!(CoreNoticeTitleUnauthorized, source = source),
            msg!(CoreNoticeDetailUnauthorized, last_state = last_state),
            NoticeAction::OpenSetup,
            None,
        ),
        Problem::NoPermission => (
            NoticeTone::Error,
            msg!(CoreNoticeTitleNoPermission, source = source),
            msg!(CoreNoticeDetailNoPermission, last_state = last_state),
            NoticeAction::OpenSetup,
            None,
        ),
        Problem::MissingRight { right, kind } => (
            NoticeTone::Error,
            msg!(CoreNoticeTitleNoPermission, source = source),
            msg!(
                CoreNoticeDetailMissingRight,
                right = right.msg(*kind),
                last_state = last_state
            ),
            NoticeAction::OpenSetup,
            None,
        ),
        Problem::RateLimited { until_ms: Some(t) } if *t > now_ms => (
            NoticeTone::Info,
            msg!(CoreNoticeTitlePaused, source = source),
            msg!(
                CoreNoticeDetailPaused,
                wait = wait_msg(*t, now_ms),
                last_state = last_state
            ),
            NoticeAction::Wait,
            Some(*t),
        ),
        Problem::RateLimited { .. } => (
            NoticeTone::Info,
            msg!(CoreNoticeTitleQuota, source = source),
            msg!(CoreNoticeDetailQuota, last_state = last_state),
            NoticeAction::RetryNow,
            None,
        ),
        Problem::Offline => (
            NoticeTone::Warning,
            msg!(CoreNoticeTitleOffline, source = source),
            msg!(CoreNoticeDetailOffline, last_state = last_state),
            NoticeAction::RetryNow,
            None,
        ),
        Problem::Other(m) => (
            NoticeTone::Warning,
            msg!(CoreNoticeTitleFailed, source = source),
            msg!(
                CoreNoticeDetailFailed,
                message = m.clone(),
                last_state = last_state
            ),
            NoticeAction::RetryNow,
            None,
        ),
    };
    Some(SourceNotice {
        source: st.source,
        tone,
        title,
        detail,
        action,
        since_ms: st.since_ms,
        retry_at_ms,
    })
}

/// Notices of all sources that have a problem.
pub fn source_notices(sync: &SyncState, now_ms: u64) -> Vec<SourceNotice> {
    sync.sources
        .iter()
        .filter_map(|s| source_notice(s, now_ms))
        .collect()
}

/// Headline and cause when the browser cannot reach the local server.
pub fn server_unreachable_title() -> Msg {
    msg!(CoreNoticeServerUnreachableTitle)
}
pub fn server_unreachable_text() -> Msg {
    msg!(CoreNoticeServerUnreachableText)
}
/// Headline and cause when the browser itself has no network.
pub fn browser_offline_title() -> Msg {
    msg!(CoreNoticeBrowserOfflineTitle)
}
pub fn browser_offline_text() -> Msg {
    msg!(CoreNoticeBrowserOfflineText)
}

/// Seconds to wait before reconnect attempt number `attempt` (0-based): 2, 4, 8, 15, 30, 30 …
pub fn reconnect_delay_s(attempt: u32) -> u32 {
    match attempt {
        0 => 2,
        1 => 4,
        2 => 8,
        3 => 15,
        _ => 30,
    }
}

/// Text for a failed request to the local backend, by HTTP status. `server_message` is the
/// text of the backend, if it sent one (already in the language of the reader); it wins for
/// client errors because it knows more.
pub fn http_failure_msg(status: u16, server_message: Option<&str>) -> Msg {
    let own = server_message
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(|m| msg!(CommonRaw, text = m));
    match status {
        401 => msg!(CoreNoticeHttp401),
        403 => own.unwrap_or_else(|| msg!(CoreNoticeHttp403)),
        429 => msg!(CoreNoticeHttp429),
        500..=599 => own.unwrap_or_else(|| msg!(CoreNoticeHttp5xx, status = status)),
        _ => own.unwrap_or_else(|| msg!(CoreNoticeHttpOther, status = status)),
    }
}

#[cfg(test)]
mod tests;
