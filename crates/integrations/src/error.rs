use std::fmt;

use cockpit_core::Problem;

/// Was bei einem Aufruf schiefgehen kann. Die Meldungen enthalten nie einen Schlüssel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Keine Verbindung, Zeitüberschreitung o. Ä.
    Network(String),
    /// 401: Schlüssel fehlt, ist abgelaufen oder widerrufen.
    NotSignedIn,
    /// 403: Schlüssel gültig, aber ohne Berechtigung.
    NoPermission,
    /// 403/429 mit aufgebrauchtem Kontingent. `warten_s`: so lange sollen wir pausieren
    /// (aus `Retry-After` bzw. dem Zeitpunkt, an dem das Kontingent zurückkommt).
    RateLimit {
        wait_s: Option<u64>,
    },
    NotFound,
    /// Der Dienst lehnt die Anfrage ab (400, 405, 409, 422) – mit seiner Meldung, falls er eine gibt.
    Rejected(String),
    /// Anderer HTTP-Status.
    Status(u16),
    /// Antwort nicht lesbar oder anders aufgebaut als erwartet.
    Response(String),
    /// Ungültige Einstellung (z. B. Adresse).
    Config(String),
    /// Der Dienst kennt keine passende Möglichkeit (z. B. keine Jira-Transition zum Ziel-Status).
    NotPossible(String),
}

impl Error {
    /// Ersetzt den Schlüssel in allen Texten durch `***`, falls ein Dienst ihn zurückspielt.
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

    /// Der Zustand für die Anzeige je Quelle. `jetzt_ms`: Uhrzeit des Aufrufers (Millisekunden).
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
        let f =
            Error::Rejected("Bad credentials ghp_geheim".into()).without_key("ghp_geheim");
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
