//! Lesende Clients für GitHub und Jira Cloud – und die Abbildung ihrer Antworten auf die
//! Typen aus `cockpit-core`.
//!
//! * [`github`]: Pull Requests, Dateien mit Patch, Kommentare, Check-Status
//! * [`jira`]: Issues per JQL, Kommentare (Atlassian Document Format → Text)
//! * [`Secret`]: ein Schlüssel, der nie in `Debug`-Ausgaben oder Logs auftaucht
//!
//! Dieses Crate **schreibt nichts** bei den Diensten. Die Abbildungsfunktionen sind reine
//! Funktionen und werden mit gespeicherten Beispiel-Antworten getestet; die Clients mit
//! einem Mock-Server. Gegen die echten Dienste ist noch nichts geprüft.

mod error;
mod http;
mod secret;
pub mod text;
pub mod zeit;

pub mod github;
pub mod jira;

pub use error::Fehler;
pub use secret::Secret;
