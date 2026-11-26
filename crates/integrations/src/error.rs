use std::fmt;

use cockpit_core::Problem;
use cockpit_i18n::{Lang, Msg, msg};

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

/// What kind of failure an [`Error`] is. Callers that react to the kind ask [`Error::cause`]
/// instead of matching the variants themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The key is missing, expired or revoked.
    Token,
    /// The key is valid, but may not do this.
    Permission,
    /// Like [`Cause::Permission`], and the error says exactly which right is missing.
    MissingRight,
    /// The quota is used up.
    RateLimit,
    /// The thing does not exist (or the key may not see it).
    NotFound,
    /// A download is larger than the limit.
    TooLarge,
    /// The service refuses the request itself (see [`Error::rejection`] for its reason).
    Rejected,
    /// The answer is not readable or not what the service sends, such as a foreign page where the
    /// API should be.
    BadResponse,
    /// An invalid setting, such as an address.
    BadConfig,
    /// No connection, timeout or similar.
    Network,
    /// Everything else: other status codes, options the service lacks, messages of this program.
    Other,
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

    /// The kind of failure; the one place that sorts the variants.
    pub fn cause(&self) -> Cause {
        match self {
            Error::NotSignedIn => Cause::Token,
            Error::NoPermission => Cause::Permission,
            Error::MissingRight { .. } => Cause::MissingRight,
            Error::RateLimit { .. } => Cause::RateLimit,
            Error::NotFound => Cause::NotFound,
            Error::TooLarge => Cause::TooLarge,
            Error::Rejected(_) => Cause::Rejected,
            Error::Response(_) => Cause::BadResponse,
            Error::Config(_) => Cause::BadConfig,
            Error::Network(_) => Cause::Network,
            Error::Status(_) | Error::NotPossible(_) | Error::Coded(_) => Cause::Other,
        }
    }

    /// What the service said when it refused the request, if it said anything.
    pub fn rejection(&self) -> Option<&str> {
        match self {
            Error::Rejected(m) if !m.is_empty() => Some(m),
            _ => None,
        }
    }

    /// The state for the display per source. `now_ms`: time of day of the caller (milliseconds).
    pub fn problem(&self, now_ms: u64) -> Problem {
        match self {
            // The two kinds that carry data of their own.
            Error::MissingRight { right, kind } => Problem::MissingRight {
                right: *right,
                kind: *kind,
            },
            Error::RateLimit { wait_s } => Problem::RateLimited {
                until_ms: wait_s.map(|s| now_ms + s * 1000),
            },
            _ => match self.cause() {
                Cause::Token => Problem::Unauthorized,
                Cause::Permission => Problem::NoPermission,
                Cause::Network => Problem::Offline,
                _ => Problem::Other(self.msg()),
            },
        }
    }
}

impl Error {
    /// The message as code and parameters, for translation.
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

/// The English text, for logs and tests; [`Error::msg`] is the translatable form.
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg().render(Lang::En))
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
