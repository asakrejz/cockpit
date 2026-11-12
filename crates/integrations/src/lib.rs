//! Clients für GitHub und Jira Cloud – und die Abbildung ihrer Antworten auf die
//! Typen aus `cockpit-core`.
//!
//! * [`github`]: Pull Requests, Dateien mit Patch, Kommentare, Check-Status
//! * [`jira`]: Issues per JQL, Kommentare (Atlassian Document Format → Text)
//! * [`litellm`]: Verbindungstest zum LiteLLM-Proxy (erreichbar, Key ok, Modell vorhanden)
//! * [`oauth`]: Anmeldung per OAuth (GitHub App, Atlassian 3LO), Refresh und Widerruf
//! * [`Secret`]: ein Schlüssel, der nie in `Debug`-Ausgaben oder Logs auftaucht
//!
//! * [`ausgehend`]: welche Aktion geht als welcher Aufruf an welchen Dienst (rein, getestet)
//! * [`abgleich`]: Abruf aller Quellen für den Abgleich im Backend
//!
//! Geschrieben wird nur über die Aktionen aus [`ausgehend`]. Die Abbildungsfunktionen sind reine
//! Funktionen und werden mit gespeicherten Beispiel-Antworten getestet; die Clients mit
//! einem Mock-Server. Gegen die echten Dienste ist noch nichts geprüft.

pub mod abgleich;
pub mod ausgehend;
mod error;
mod http;
pub mod oauth;
mod secret;
pub mod text;
pub mod zeit;

pub mod github;
pub mod jira;
pub mod litellm;

pub use abgleich::{Dienste, GitHubDienst, JiraDienst};
pub use error::Fehler;
pub use secret::Secret;
