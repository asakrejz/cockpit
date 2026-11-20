use std::fmt;

use cockpit_core::Problem;
use cockpit_i18n::{Msg, msg};

/// What can go wrong in a call. The messages never contain a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No connection, timeout or similar.
    Network(String),
    /// 401: key missing, expired or revoked.
    NotSignedIn,
    /// 403: key valid, but without permission.
    NoPermission,
    /// 403/429 with exhausted quota. `wait_s`: how long we should pause
    /// (from `Retry-After` or the time at which the quota comes back).
    RateLimit {
        wait_s: Option<u64>,
    },
    NotFound,
    /// A download is larger than the limit.
    TooLarge,
    /// The service rejects the request (400, 405, 409, 422) – with its message, if it gives one.
    Rejected(String),
    /// Other HTTP status.
    Status(u16),
    /// Response not readable or structured differently than expected.
    Response(String),
    /// Invalid setting (e.g. address).
    Config(String),
    /// The service has no matching option (e.g. no Jira transition to the target status).
    NotPossible(String),
    /// A message of this program with a code, shown to people in their language.
    Coded(Msg),
    /// GitHub refused because the credential lacks a right (a missing scope, or a permission
    /// of a fine-grained token or GitHub App). The message says exactly which one.
    MissingRight {
        right: cockpit_core::Right,
        kind: cockpit_core::TokenKind,
    },
}

impl Error {
    /// Replaces the key in all texts with `***`, in case a service echoes it back.
    pub fn without_key(self, key: &str) -> Self {
        if key.is_empty() {
            return self;
        }
        let pure = |m: String| m.replace(key, "***");
        match self {
            Error::Network(m) => Error::Network(pure(m)),
            Error::Rejected(m) => Error::Rejected(pure(m)),
            Error::Response(m) => Error::Response(pure(m)),
            Error::Config(m) => Error::Config(pure(m)),
            Error::NotPossible(m) => Error::NotPossible(pure(m)),
            Error::Coded(m) => Error::Coded(pure_msg(m, key)),
            other => other,
        }
    }

    /// The state for the display per source. `now_ms`: time of day of the caller (milliseconds).
    pub fn problem(&self, now_ms: u64) -> Problem {
        match self {
            Error::NotSignedIn => Problem::Unauthorized,
            Error::NoPermission => Problem::NoPermission,
            Error::MissingRight { right, kind } => Problem::MissingRight {
                right: *right,
                kind: *kind,
            },
            Error::RateLimit { wait_s } => Problem::RateLimited {
                until_ms: wait_s.map(|s| now_ms + s * 1000),
            },
            Error::Network(_) => Problem::Offline,
            other => Problem::Other(other.to_string()),
        }
    }
}

impl Error {
    /// The message as code and parameters; the UI renders it in the chosen language.
    pub fn msg(&self) -> Msg {
        match self {
            Error::Network(m) => msg!(IntegrationsNetwork, reason = Msg::raw(m)),
            Error::NotSignedIn => msg!(IntegrationsNotSignedIn),
            Error::NoPermission => msg!(IntegrationsNoPermission),
            Error::RateLimit { .. } => msg!(IntegrationsRateLimit),
            Error::NotFound => msg!(IntegrationsNotFound),
            Error::TooLarge => msg!(IntegrationsTooLarge),
            Error::Rejected(m) if m.is_empty() => msg!(IntegrationsRejected),
            Error::Rejected(m) => msg!(IntegrationsRejectedWith, reason = Msg::raw(m)),
            Error::Status(code @ 500..=599) => msg!(IntegrationsServerError, code = *code),
            Error::Status(code) => msg!(IntegrationsUnexpectedStatus, code = *code),
            Error::Response(m) => msg!(IntegrationsBadResponse, reason = Msg::raw(m)),
            Error::Config(m) => msg!(IntegrationsBadConfig, reason = Msg::raw(m)),
            Error::NotPossible(m) => Msg::raw(m),
            Error::Coded(m) => m.clone(),
            Error::MissingRight { right, kind } => right.msg(*kind),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg().de())
    }
}

impl std::error::Error for Error {}

/// Replaces the key in all parameters of a message.
fn pure_msg(mut m: Msg, key: &str) -> Msg {
    use cockpit_i18n::ArgValue;
    fn clean(v: ArgValue, key: &str) -> ArgValue {
        match v {
            ArgValue::Text(t) => ArgValue::Text(t.replace(key, "***")),
            ArgValue::Msg(m) => ArgValue::Msg(pure_msg(m, key)),
            ArgValue::List { items, sep } => ArgValue::List {
                items: items.into_iter().map(|i| clean(i, key)).collect(),
                sep,
            },
        }
    }
    m.args = m
        .args
        .into_iter()
        .map(|(k, v)| (k, clean(v, key)))
        .collect();
    m
}

#[cfg(test)]
mod tests;
