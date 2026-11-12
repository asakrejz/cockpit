//! Clients für GitHub und Jira Cloud – und die Abbildung ihrer Antworten auf die
//! Typen aus `cockpit-core`.
//!
//! * [`github`]: Pull Requests, Dateien mit Patch, Kommentare, Check-Status
//! * [`jira`]: Issues per JQL, Kommentare (Atlassian Document Format → Text)
//! * [`endpunkt`]: Verbindungstest und Modellliste des Modell-Endpunkts (Proxy wie LiteLLM oder direkt bei Anthropic)
//! * [`oauth`]: Anmeldung per OAuth (GitHub App, Atlassian 3LO), Refresh und Widerruf
//! * [`Secret`]: ein Schlüssel, der nie in `Debug`-Ausgaben oder Logs auftaucht
//!
//! * [`ausgehend`]: welche Aktion geht als welcher Aufruf an welchen Dienst (rein, getestet)
//! * [`abgleich`]: Abruf aller Quellen für den Abgleich im Backend
//!
//! Geschrieben wird nur über die Aktionen aus [`ausgehend`]. Die Abbildungsfunktionen sind reine
//! Funktionen und werden mit gespeicherten Beispiel-Antworten getestet; die Clients mit
//! einem Mock-Server. Gegen die echten Dienste ist noch nichts geprüft.

pub mod sync;
pub mod outbound;
mod error;
mod http;
pub mod oauth;
mod secret;
pub mod text;
pub mod time;

pub mod endpoint;
pub mod github;
pub mod jira;

pub use sync::{Services, GitHubService, JiraService};
pub use error::Error;
pub use secret::Secret;
