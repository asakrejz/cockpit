use std::fmt;

use cockpit_core::Problem;

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
            other => other,
        }
    }

    /// The state for the display per source. `now_ms`: time of day of the caller (milliseconds).
    pub fn problem(&self, now_ms: u64) -> Problem {
        match self {
            Error::NotSignedIn => Problem::Unauthorized,
            Error::NoPermission => Problem::NoPermission,
            Error::RateLimit { wait_s } => Problem::RateLimited {
                until_ms: wait_s.map(|s| now_ms + s * 1000),
            },
            Error::Network(_) => Problem::Offline,
            other => Problem::Other(other.to_string()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(m) => write!(f, "Keine Verbindung zum Dienst: {m}"),
            Error::NotSignedIn => {
                write!(
                    f,
                    "Nicht angemeldet: Schlüssel fehlt, ist abgelaufen oder widerrufen"
                )
            }
            Error::NoPermission => write!(f, "Dem Schlüssel fehlt die Berechtigung"),
            Error::RateLimit { .. } => write!(
                f,
                "Kontingent des Dienstes aufgebraucht, später erneut versuchen"
            ),
            Error::NotFound => write!(f, "Nicht gefunden"),
            Error::Rejected(m) if m.is_empty() => write!(f, "Der Dienst lehnt die Anfrage ab"),
            Error::Rejected(m) => write!(f, "Der Dienst lehnt die Anfrage ab: {m}"),
            Error::Status(code) => write!(f, "Unerwarteter Status vom Dienst: {code}"),
            Error::Response(m) => write!(f, "Antwort des Dienstes nicht lesbar: {m}"),
            Error::Config(m) => write!(f, "Ungültige Einstellung: {m}"),
            Error::NotPossible(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_removed_from_messages() {
        let f = Error::Rejected("Bad credentials ghp_geheim".into()).without_key("ghp_geheim");
        assert!(!f.to_string().contains("ghp_geheim"));
        assert!(f.to_string().contains("***"));
    }

    #[test]
    fn problem_per_error() {
        assert_eq!(Error::NotSignedIn.problem(0), Problem::Unauthorized);
        assert_eq!(Error::NoPermission.problem(0), Problem::NoPermission);
        assert_eq!(Error::Network("x".into()).problem(0), Problem::Offline);
        assert_eq!(
            Error::RateLimit { wait_s: Some(30) }.problem(1000),
            Problem::RateLimited {
                until_ms: Some(31_000)
            }
        );
        assert_eq!(
            Error::RateLimit { wait_s: None }.problem(1000),
            Problem::RateLimited { until_ms: None }
        );
        assert!(matches!(Error::Status(500).problem(0), Problem::Other(_)));
    }
}
