//! Fachlogik des Cockpits – ohne UI und ohne Netzwerk.
//!
//! * [`model`]: Tickets, Pull Requests und warum sie Aufmerksamkeit brauchen
//! * [`review`]: Diff, Vorschläge des Agenten, Freigabe und Urteil
//! * [`agent`]: der Lauf eines Agenten als Zustandsmaschine
//! * [`connections`]: Verbindungen, Berechtigungen und Eingabeprüfung
//! * [`search`]: Suche und Filter der Inbox
//! * [`workspace`]: alles zusammen, mit den Aktionen der Oberfläche
//! * [`demo`]: Beispieldaten, solange kein Backend angebunden ist
//!
//! Jede Aktion der Oberfläche ist eine Methode auf [`Workspace`] und damit mit
//! `cargo test` prüfbar. Ein echtes Backend würde dieselben Aktionen gegen Jira,
//! GitHub und den Agenten ausführen und das Ergebnis in den Workspace schreiben.

pub mod agent;
pub mod connections;
pub mod demo;
pub mod model;
pub mod review;
pub mod search;
pub mod text;
pub mod workspace;

pub use agent::{AgentOptions, AgentRun, RunState, Step, StepState};
pub use connections::Connections;
pub use model::*;
pub use review::{
    Decision, DiffLine, FileDiff, Level, LineComment, LineKind, Review, SendError, Suggestion,
    Verdict,
};
pub use workspace::{AgentError, Workspace};
