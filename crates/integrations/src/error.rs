use std::fmt;

/// Was beim Abruf schiefgehen kann. Die Meldungen enthalten nie einen Schlüssel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fehler {
    /// Keine Verbindung, Zeitüberschreitung o. Ä.
    Netzwerk(String),
    /// 401: Schlüssel fehlt, ist abgelaufen oder widerrufen.
    NichtAngemeldet,
    /// 403: Schlüssel gültig, aber ohne Berechtigung.
    KeineBerechtigung,
    /// 403/429 mit aufgebrauchtem Kontingent.
    RateLimit,
    NichtGefunden,
    /// Anderer HTTP-Status.
    Status(u16),
    /// Antwort nicht lesbar oder anders aufgebaut als erwartet.
    Antwort(String),
    /// Ungültige Einstellung (z. B. Adresse).
    Einstellung(String),
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
            Fehler::RateLimit => write!(
                f,
                "Kontingent des Dienstes aufgebraucht, später erneut versuchen"
            ),
            Fehler::NichtGefunden => write!(f, "Nicht gefunden"),
            Fehler::Status(code) => write!(f, "Unerwarteter Status vom Dienst: {code}"),
            Fehler::Antwort(m) => write!(f, "Antwort des Dienstes nicht lesbar: {m}"),
            Fehler::Einstellung(m) => write!(f, "Ungültige Einstellung: {m}"),
        }
    }
}

impl std::error::Error for Fehler {}
