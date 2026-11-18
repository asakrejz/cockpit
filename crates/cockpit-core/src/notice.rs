//! Plain-language states for the interface: what is wrong with a source, how long it
//! lasts and what to do next. One place for the wording, so pages do not invent their own.
//!
//! Pure functions on the data the workspace already carries ([`SyncState`]); the browser only
//! renders the result. No message contains a key: problems come from [`Problem`], which is
//! free of secrets by construction.

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
    /// Go to "Einrichten" and renew the token.
    OpenSetup,
    /// Try again now ("Jetzt abgleichen").
    RetryNow,
    /// Nothing to do, the sync resumes by itself.
    Wait,
}

/// A banner for one source that is not healthy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceNotice {
    pub source: Source,
    pub tone: NoticeTone,
    /// Short headline, e.g. "GitHub nicht erreichbar".
    pub title: String,
    /// Cause, how long the data is old and what to do.
    pub detail: String,
    pub action: NoticeAction,
    /// Since when the problem lasts.
    pub since_ms: Option<u64>,
    /// When the sync tries again (rate limit only).
    pub retry_at_ms: Option<u64>,
}

/// "gerade eben", "vor 5 Min.", "vor 2 Std.", "vor 3 Tagen".
pub fn ago(then_ms: u64, now_ms: u64) -> String {
    let s = now_ms.saturating_sub(then_ms) / 1000;
    match s {
        0..=59 => "gerade eben".into(),
        60..=3599 => format!("vor {} Min.", s / 60),
        3600..=86_399 => format!("vor {} Std.", s / 3600),
        _ => format!("vor {} Tagen", s / 86_400),
    }
}

/// "in 20 Sek.", "in 5 Min.", "in 2 Std." – the wait until `until_ms`.
pub fn wait_text(until_ms: u64, now_ms: u64) -> String {
    let s = until_ms.saturating_sub(now_ms).div_ceil(1000);
    match s {
        0 => "gleich".into(),
        1..=59 => format!("in {s} Sek."),
        60..=3599 => format!("in {} Min.", s.div_ceil(60)),
        _ => format!("in {} Std.", s.div_ceil(3600)),
    }
}

fn name(source: Source) -> &'static str {
    match source {
        Source::Jira => "Jira",
        Source::GitHub => "GitHub",
    }
}

fn stand(st: &SourceState, now_ms: u64) -> String {
    match st.last_ok_ms {
        Some(t) => {
            let when = ago(t, now_ms);
            let dot = if when.ends_with('.') { "" } else { "." };
            format!("Letzter Stand: {when}{dot}")
        }
        None => "Es gibt noch keinen abgeglichenen Stand.".into(),
    }
}

/// The notice for one source, `None` while it is healthy.
pub fn source_notice(st: &SourceState, now_ms: u64) -> Option<SourceNotice> {
    let problem = st.problem.as_ref()?;
    let n = name(st.source);
    let (tone, title, detail, action, retry_at_ms) = match problem {
        Problem::Unauthorized => (
            NoticeTone::Error,
            format!("{n}: Token abgelehnt"),
            format!(
                "Der Token ist abgelaufen oder wurde widerrufen. Erneuere ihn unter „Einrichten“. {}",
                stand(st, now_ms)
            ),
            NoticeAction::OpenSetup,
            None,
        ),
        Problem::NoPermission => (
            NoticeTone::Error,
            format!("{n}: Berechtigung fehlt"),
            format!(
                "Der Token ist gültig, darf das aber nicht lesen. Prüfe unter „Einrichten“, welche Rechte er braucht. {}",
                stand(st, now_ms)
            ),
            NoticeAction::OpenSetup,
            None,
        ),
        Problem::RateLimited { until_ms: Some(t) } if *t > now_ms => (
            NoticeTone::Info,
            format!("{n}: Abgleich pausiert"),
            format!(
                "Das Kontingent des Dienstes ist aufgebraucht. Cockpit fragt {} wieder an. {}",
                wait_text(*t, now_ms),
                stand(st, now_ms)
            ),
            NoticeAction::Wait,
            Some(*t),
        ),
        Problem::RateLimited { .. } => (
            NoticeTone::Info,
            format!("{n}: Kontingent aufgebraucht"),
            format!(
                "Der Dienst bremst Anfragen. Cockpit versucht es beim nächsten Abgleich erneut. {}",
                stand(st, now_ms)
            ),
            NoticeAction::RetryNow,
            None,
        ),
        Problem::Offline => (
            NoticeTone::Warning,
            format!("{n} nicht erreichbar"),
            format!(
                "Der Dienst antwortet nicht rechtzeitig (oder das Netz ist weg). {} Cockpit versucht es weiter.",
                stand(st, now_ms)
            ),
            NoticeAction::RetryNow,
            None,
        ),
        Problem::Other(m) => (
            NoticeTone::Warning,
            format!("{n}: Abgleich fehlgeschlagen"),
            format!("{m}. {}", stand(st, now_ms)),
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
pub const SERVER_UNREACHABLE_TITLE: &str = "Der Server ist nicht erreichbar";
pub const SERVER_UNREACHABLE_TEXT: &str = "Cockpit versucht es automatisch erneut. Läuft der Server noch? Starte ihn sonst neu (start-cockpit.command oder scripts/start.sh).";
/// Headline and cause when the browser itself has no network.
pub const BROWSER_OFFLINE_TITLE: &str = "Du bist offline";
pub const BROWSER_OFFLINE_TEXT: &str = "Dieser Rechner hat gerade kein Netz. Lokale Daten bleiben sichtbar, Jira und GitHub werden wieder abgeglichen, sobald das Netz zurück ist.";

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
/// German text of the backend, if it sent one; it wins for client errors because it knows more.
pub fn http_failure_text(status: u16, server_message: Option<&str>) -> String {
    let msg = server_message.map(str::trim).filter(|m| !m.is_empty());
    match status {
        401 => "Die Anfrage wurde abgelehnt – Anmeldung oder Token prüfen (unter „Einrichten“)".into(),
        403 => msg.map_or_else(
            || "Das ist nicht erlaubt – die Berechtigung fehlt".into(),
            str::to_string,
        ),
        429 => "Zu viele Anfragen – bitte kurz warten und noch einmal versuchen".into(),
        500..=599 => msg.map_or_else(
            || format!("Der Server hat einen Fehler gemeldet ({status}). Versuche es gleich noch einmal"),
            str::to_string,
        ),
        _ => msg.map_or_else(
            || format!("Der Server antwortet mit Fehler {status}"),
            str::to_string,
        ),
    }
}

#[cfg(test)]
mod tests;
