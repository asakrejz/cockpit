use std::fmt;

use cockpit_core::Problem;

/// Was bei einem Aufruf schiefgehen kann. Die Meldungen enthalten nie einen Schlüssel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fehler {
    /// Keine Verbindung, Zeitüberschreitung o. Ä.
    Netzwerk(String),
    /// 401: Schlüssel fehlt, ist abgelaufen oder widerrufen.
    NichtAngemeldet,
    /// 403: Schlüssel gültig, aber ohne Berechtigung.
    KeineBerechtigung,
    /// 403/429 mit aufgebrauchtem Kontingent. `warten_s`: so lange sollen wir pausieren
    /// (aus `Retry-After` bzw. dem Zeitpunkt, an dem das Kontingent zurückkommt).
    RateLimit {
        warten_s: Option<u64>,
    },
    NichtGefunden,
    /// Der Dienst lehnt die Anfrage ab (400, 405, 409, 422) – mit seiner Meldung, falls er eine gibt.
    Abgelehnt(String),
    /// Anderer HTTP-Status.
    Status(u16),
    /// Antwort nicht lesbar oder anders aufgebaut als erwartet.
    Antwort(String),
    /// Ungültige Einstellung (z. B. Adresse).
    Einstellung(String),
    /// Der Dienst kennt keine passende Möglichkeit (z. B. keine Jira-Transition zum Ziel-Status).
    NichtMoeglich(String),
}

impl Fehler {
    /// Ersetzt den Schlüssel in allen Texten durch `***`, falls ein Dienst ihn zurückspielt.
    pub fn ohne_schluessel(self, schluessel: &str) -> Self {
        if schluessel.is_empty() {
            return self;
        }
        let rein = |m: String| m.replace(schluessel, "***");
        match self {
            Fehler::Netzwerk(m) => Fehler::Netzwerk(rein(m)),
            Fehler::Abgelehnt(m) => Fehler::Abgelehnt(rein(m)),
            Fehler::Antwort(m) => Fehler::Antwort(rein(m)),
            Fehler::Einstellung(m) => Fehler::Einstellung(rein(m)),
            Fehler::NichtMoeglich(m) => Fehler::NichtMoeglich(rein(m)),
            sonst => sonst,
        }
    }

    /// Der Zustand für die Anzeige je Quelle. `jetzt_ms`: Uhrzeit des Aufrufers (Millisekunden).
    pub fn problem(&self, jetzt_ms: u64) -> Problem {
        match self {
            Fehler::NichtAngemeldet => Problem::Unauthorized,
            Fehler::KeineBerechtigung => Problem::NoPermission,
            Fehler::RateLimit { warten_s } => Problem::RateLimited {
                until_ms: warten_s.map(|s| jetzt_ms + s * 1000),
            },
            Fehler::Netzwerk(_) => Problem::Offline,
            andere => Problem::Other(andere.to_string()),
        }
    }
}

impl fmt::Display for Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fehler::Netzwerk(m) => write!(f, "Keine Verbindung zum Dienst: {m}"),
            Fehler::NichtAngemeldet => {
                write!(
                    f,
                    "Nicht angemeldet: Schlüssel fehlt, ist abgelaufen oder widerrufen"
                )
            }
            Fehler::KeineBerechtigung => write!(f, "Dem Schlüssel fehlt die Berechtigung"),
            Fehler::RateLimit { .. } => write!(
                f,
                "Kontingent des Dienstes aufgebraucht, später erneut versuchen"
            ),
            Fehler::NichtGefunden => write!(f, "Nicht gefunden"),
            Fehler::Abgelehnt(m) if m.is_empty() => write!(f, "Der Dienst lehnt die Anfrage ab"),
            Fehler::Abgelehnt(m) => write!(f, "Der Dienst lehnt die Anfrage ab: {m}"),
            Fehler::Status(code) => write!(f, "Unerwarteter Status vom Dienst: {code}"),
            Fehler::Antwort(m) => write!(f, "Antwort des Dienstes nicht lesbar: {m}"),
            Fehler::Einstellung(m) => write!(f, "Ungültige Einstellung: {m}"),
            Fehler::NichtMoeglich(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Fehler {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schluessel_wird_aus_meldungen_entfernt() {
        let f =
            Fehler::Abgelehnt("Bad credentials ghp_geheim".into()).ohne_schluessel("ghp_geheim");
        assert!(!f.to_string().contains("ghp_geheim"));
        assert!(f.to_string().contains("***"));
    }

    #[test]
    fn problem_je_fehler() {
        assert_eq!(Fehler::NichtAngemeldet.problem(0), Problem::Unauthorized);
        assert_eq!(Fehler::KeineBerechtigung.problem(0), Problem::NoPermission);
        assert_eq!(Fehler::Netzwerk("x".into()).problem(0), Problem::Offline);
        assert_eq!(
            Fehler::RateLimit { warten_s: Some(30) }.problem(1000),
            Problem::RateLimited {
                until_ms: Some(31_000)
            }
        );
        assert_eq!(
            Fehler::RateLimit { warten_s: None }.problem(1000),
            Problem::RateLimited { until_ms: None }
        );
        assert!(matches!(Fehler::Status(500).problem(0), Problem::Other(_)));
    }
}
