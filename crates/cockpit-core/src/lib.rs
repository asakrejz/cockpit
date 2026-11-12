//! Fachlogik des Cockpits – ohne UI und ohne Netzwerk.
//!
//! * [`model`]: Tickets, Pull Requests und warum sie Aufmerksamkeit brauchen
//! * [`attention`]: Regeln, warum ein Eintrag Aufmerksamkeit braucht, und Zurückstellen
//! * [`review`]: Diff, Vorschläge des Agenten, Freigabe und Urteil
//! * [`agent`]: der Lauf eines Agenten als Zustandsmaschine
//! * [`diagnosis`]: Ursache und Fix-Vorschlag zu einem roten Check
//! * [`connections`]: Verbindungen, Berechtigungen und Eingabeprüfung
//! * [`link`]: Ticket und Pull Request über den Ticket-Schlüssel verknüpfen
//! * [`search`]: Suche und Filter der Inbox
//! * [`notify`]: Ereignisse für Benachrichtigungen, Kanäle und Ruhezeiten
//! * [`usage`]: Verbrauch, Kosten, Grenzen und das Kassenbuch der Agenten
//! * [`rules`]: Regeln pro Repository (Pfade, Pflicht-Tests, Anweisungen)
//! * [`sync`]: Abgleich mit Jira und GitHub (reine Zusammenführung)
//! * [`team`]: Auslastung pro Person und was gerade hängt
//! * [`workspace`]: alles zusammen, mit den Aktionen der Oberfläche
//! * [`action`]: [`Action`], `Workspace::apply` und das Protokoll
//! * [`demo`]: Beispieldaten, solange kein Backend angebunden ist
//! * [`api`]: die JSON-Formen zwischen App und Backend (Server-Modus)
//! * [`persist`]: Speichern und Laden des Workspace (JSON mit Versionsnummer)
//!
//! Jede Aktion der Oberfläche ist eine [`Action`], die `Workspace::apply` über Methoden
//! auf [`Workspace`] ausführt und protokolliert – und damit mit `cargo test` prüfbar. Ein echtes Backend würde dieselben Aktionen gegen Jira,
//! GitHub und den Agenten ausführen und das Ergebnis in den Workspace schreiben.

pub mod action;
pub mod agent;
pub mod api;
pub mod attention;
pub mod connections;
pub mod demo;
pub mod diagnosis;
pub mod link;
pub mod model;
pub mod notify;
pub mod persist;
pub mod review;
pub mod rules;
pub mod search;
pub mod sync;
pub mod team;
pub mod text;
pub mod usage;
pub mod workspace;

pub use action::{Action, ActionError, Actor, Applied, LogEntry, MAX_LOG, Target};
pub use agent::{
    AgentOptions, AgentRun, MessageKind, Plan, Question, RunKind, RunMessage, RunState, Speaker,
    Step, StepState, Tick,
};
pub use attention::{Hit, Rule, Rules, Snooze, SnoozeChoice};
pub use connections::{AuthKind, Connections, ServiceStatus};
pub use diagnosis::{Diagnosis, Fix};
pub use model::*;
pub use notify::{
    Channel, Channels, Delivery, Event as NotifyEvent, NotifyReason, NotifySettings, QuietHours,
};
pub use review::{
    CHECK_STEPS, CheckRun, Decision, DiffLine, FileDiff, Level, LineComment, LineKind,
    MergeBlocker, MergeMethod, Review, SendError, Suggestion, SuggestionDiff, Verdict,
    merge_blockers,
};
pub use rules::{DEMO_REPO, PathViolation, RepoRules, ViolationKind, glob_match};
pub use search::InboxFilter;
pub use sync::{
    Change, Loaded, Outcome, PrLink, Problem, Remote, SourceData, SourceState, SyncState,
};
pub use team::{CAPACITY, LoadLevel, Member, Stuck, StuckKind, state_label};
pub use usage::{AgentLimits, Charge, Pricing, StopReason, Usage};
pub use workspace::{AgentError, LinkError, ResumeError, StatusChange, Workspace};
