//! Datenmodell: Einträge der Inbox und was dazugehört.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Jira,
    GitHub,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Jira => "Jira",
            Source::GitHub => "GitHub",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ticket,
    PullRequest,
}

/// Gehört der Eintrag mir oder jemandem im Team?
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Mine,
    Team,
}

/// Filter der Inbox.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Mine,
    Team,
    /// Zurückgestellte Einträge (siehe [`crate::attention`]).
    Snoozed,
}

impl Filter {
    /// ID und Beschriftung, in Anzeigereihenfolge.
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("alle", "Alle"),
        ("ich", "Ich"),
        ("team", "Team"),
        ("zurueckgestellt", "Zurückgestellt"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            Filter::All => "alle",
            Filter::Mine => "ich",
            Filter::Team => "team",
            Filter::Snoozed => "zurueckgestellt",
        }
    }

    pub fn from_id(id: &str) -> Option<Filter> {
        [Filter::All, Filter::Mine, Filter::Team, Filter::Snoozed]
            .into_iter()
            .find(|f| f.id() == id)
    }

    pub fn matches(self, scope: Scope) -> bool {
        match self {
            Filter::All | Filter::Snoozed => true,
            Filter::Mine => scope == Scope::Mine,
            Filter::Team => scope == Scope::Team,
        }
    }
}

/// Filter der Inbox nach Quelle: alles, nur Jira oder nur GitHub.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceFilter {
    #[default]
    All,
    Jira,
    GitHub,
}

impl SourceFilter {
    /// ID und Beschriftung, in Anzeigereihenfolge.
    pub const OPTIONS: &'static [(&'static str, &'static str)] =
        &[("alle", "Alle"), ("jira", "Jira"), ("github", "GitHub")];

    pub fn id(self) -> &'static str {
        match self {
            SourceFilter::All => "alle",
            SourceFilter::Jira => "jira",
            SourceFilter::GitHub => "github",
        }
    }

    pub fn from_id(id: &str) -> Option<SourceFilter> {
        [SourceFilter::All, SourceFilter::Jira, SourceFilter::GitHub]
            .into_iter()
            .find(|f| f.id() == id)
    }

    pub fn matches(self, source: Source) -> bool {
        match self {
            SourceFilter::All => true,
            SourceFilter::Jira => source == Source::Jira,
            SourceFilter::GitHub => source == Source::GitHub,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TicketStatus {
    #[default]
    Open,
    InProgress,
    Review,
    Done,
}

impl TicketStatus {
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("offen", "Offen"),
        ("arbeit", "In Arbeit"),
        ("review", "Review"),
        ("fertig", "Fertig"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            TicketStatus::Open => "offen",
            TicketStatus::InProgress => "arbeit",
            TicketStatus::Review => "review",
            TicketStatus::Done => "fertig",
        }
    }

    pub fn from_id(id: &str) -> Option<TicketStatus> {
        [
            TicketStatus::Open,
            TicketStatus::InProgress,
            TicketStatus::Review,
            TicketStatus::Done,
        ]
        .into_iter()
        .find(|s| s.id() == id)
    }
}

/// Wie wichtig ein Ticket ist. Alte Stände ohne Angabe gelten als „Mittel“.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Priority {
    Highest,
    High,
    #[default]
    Medium,
    Low,
    Lowest,
}

impl Priority {
    pub const ALL: [Priority; 5] = [
        Priority::Highest,
        Priority::High,
        Priority::Medium,
        Priority::Low,
        Priority::Lowest,
    ];

    /// ID und Beschriftung, in Anzeigereihenfolge (wichtigste zuerst).
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("hoechste", "Höchste"),
        ("hoch", "Hoch"),
        ("mittel", "Mittel"),
        ("niedrig", "Niedrig"),
        ("niedrigste", "Niedrigste"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            Priority::Highest => "hoechste",
            Priority::High => "hoch",
            Priority::Medium => "mittel",
            Priority::Low => "niedrig",
            Priority::Lowest => "niedrigste",
        }
    }

    pub fn from_id(id: &str) -> Option<Priority> {
        Priority::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn label(self) -> &'static str {
        Priority::OPTIONS
            .iter()
            .find(|(id, _)| *id == self.id())
            .map_or("", |(_, label)| label)
    }
}

/// Farbton für Status-Punkte und Icons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Gray,
    Blue,
    Green,
    Orange,
    Red,
    Indigo,
}

/// Warum ein Eintrag Aufmerksamkeit braucht.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasonKind {
    Comment,
    /// Jemand hat mich in einem Kommentar mit `@Name` erwähnt.
    Mention,
    ReviewRequested,
    AgentFinished,
    ChecksFailed,
    /// Für meinen Pull Request wurden Änderungen angefordert.
    ChangesRequested,
    /// Das Fälligkeitsdatum eines Tickets liegt in der Vergangenheit.
    Overdue,
    Assigned,
}

impl ReasonKind {
    pub fn tone(self) -> Tone {
        match self {
            ReasonKind::Comment => Tone::Blue,
            ReasonKind::Mention => Tone::Blue,
            ReasonKind::ReviewRequested => Tone::Orange,
            ReasonKind::AgentFinished => Tone::Green,
            ReasonKind::ChecksFailed | ReasonKind::ChangesRequested => Tone::Red,
            ReasonKind::Overdue => Tone::Orange,
            ReasonKind::Assigned => Tone::Indigo,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Person {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub initials: String,
}

impl Person {
    pub fn new(name: &str, initials: &str) -> Self {
        Self {
            name: name.into(),
            initials: initials.into(),
        }
    }

    /// Der angemeldete Nutzer.
    pub fn me() -> Self {
        Self::new("Du", "AS")
    }

    pub fn agent() -> Self {
        Self::new("Agent", "AG")
    }

    pub fn is_agent(&self) -> bool {
        *self == Self::agent()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub author: Person,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub text: String,
    /// Von mir geschrieben.
    #[serde(default)]
    pub own: bool,
}

impl Comment {
    pub fn new(author: Person, time: &str, text: &str) -> Self {
        Self {
            author,
            time: time.into(),
            text: text.into(),
            own: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checks {
    Passed,
    Failed,
    Running,
}

impl Checks {
    pub fn label(self) -> &'static str {
        match self {
            Checks::Passed => "Checks bestanden",
            Checks::Failed => "Checks fehlgeschlagen",
            Checks::Running => "Checks laufen",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Checks::Passed => Tone::Green,
            Checks::Failed => Tone::Red,
            Checks::Running => Tone::Blue,
        }
    }
}

/// Verweis auf einen Pull Request, wie er an einem Eintrag hängt.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrRef {
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub draft: bool,
    pub checks: Checks,
    /// Ein Reviewer hat Änderungen angefordert.
    #[serde(default)]
    pub changes_requested: bool,
    /// Schon gemergt.
    #[serde(default)]
    pub merged: bool,
    /// Ein Review ist angefragt.
    #[serde(default)]
    pub review_requested: bool,
}

impl PrRef {
    /// Welchen Ticket-Status dieser Pull Request nahelegt: gemergt → Erledigt, Review
    /// angefragt → Review, sonst (Entwurf oder offen) → In Arbeit.
    pub fn target_status(&self) -> TicketStatus {
        if self.merged {
            TicketStatus::Done
        } else if self.review_requested {
            TicketStatus::Review
        } else {
            TicketStatus::InProgress
        }
    }

    /// `#87 · offen`
    pub fn label(&self) -> String {
        let state = match (self.merged, self.draft) {
            (true, _) => "gemergt",
            (false, true) => "Entwurf",
            (false, false) => "offen",
        };
        format!("#{} · {state}", self.number)
    }

    /// Statuszeile für Pull Requests, bei denen ein Review ansteht.
    pub fn status(&self) -> (String, Tone) {
        if self.merged {
            return ("Gemergt".into(), Tone::Indigo);
        }
        match (self.checks, self.draft) {
            (Checks::Passed, false) => ("Review offen · Checks bestanden".into(), Tone::Orange),
            (Checks::Passed, true) => ("Entwurf · Checks bestanden".into(), Tone::Gray),
            (checks, _) => (checks.label().into(), checks.tone()),
        }
    }
}

/// Ein Eintrag der Inbox: ein Jira-Ticket oder ein GitHub Pull Request.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Interne ID, z. B. `bau142`.
    #[serde(default)]
    pub id: String,
    pub kind: Kind,
    pub source: Source,
    /// Sichtbarer Schlüssel: `BAU-142` oder `#87`.
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub reason: ReasonKind,
    #[serde(default)]
    pub reason_text: String,
    #[serde(default)]
    pub time: String,
    pub scope: Scope,
    pub owner: Person,
    /// Nur für Tickets von Bedeutung.
    #[serde(default)]
    pub status: TicketStatus,
    /// Nur für Tickets von Bedeutung.
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub pr: Option<PrRef>,
    /// Hinweis des Agenten, z. B. „Vom Agenten umgesetzt · Tests bestanden“.
    #[serde(default)]
    pub agent_note: Option<String>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// Erledigt – braucht keine Aufmerksamkeit mehr.
    #[serde(default)]
    pub read: bool,
    /// Fälligkeitsdatum als Tag (`2026-11-09`), nur für Tickets.
    #[serde(default)]
    pub due: Option<String>,
    /// Das Repo (`owner/name`) eines Pull Requests; bei Tickets leer.
    #[serde(default)]
    pub repo: Option<String>,
    /// Labels des Dienstes (Jira-Labels, GitHub-Labels).
    #[serde(default)]
    pub labels: Vec<String>,
}

impl Item {
    pub fn is_ticket(&self) -> bool {
        self.kind == Kind::Ticket
    }

    /// Das Jira-Projekt eines Tickets: der Teil des Schlüssels vor dem Bindestrich
    /// (`BAU-142` → `BAU`). Pull Requests und andere Einträge haben keines.
    ///
    /// ```
    /// use cockpit_core::demo;
    /// let ws = demo::workspace();
    /// assert_eq!(ws.item_by_key("BAU-142").unwrap().project(), Some("BAU"));
    /// assert_eq!(ws.item_by_key("#87").unwrap().project(), None);
    /// ```
    pub fn project(&self) -> Option<&str> {
        if self.source != Source::Jira {
            return None;
        }
        self.key
            .split_once('-')
            .map(|(p, _)| p)
            .filter(|p| is_project_key(p))
    }

    /// Der Status für Filter: bei Tickets der eigene, bei Pull Requests der, den der Pull
    /// Request nahelegt (gemergt → Fertig, Review angefragt → Review, sonst In Arbeit).
    pub fn effective_status(&self) -> TicketStatus {
        match (&self.kind, &self.pr) {
            (Kind::PullRequest, Some(pr)) => pr.target_status(),
            _ => self.status,
        }
    }
}

/// Ist das ein gültiger Jira-Projektschlüssel? Ein Buchstabe vorn, dann Buchstaben, Ziffern
/// oder `_`. So rutscht nichts in eine JQL-Abfrage.
pub fn is_project_key(key: &str) -> bool {
    key.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_options_agree() {
        for (id, _) in Filter::OPTIONS {
            assert_eq!(Filter::from_id(id).map(Filter::id), Some(*id));
        }
        for (id, _) in TicketStatus::OPTIONS {
            assert_eq!(TicketStatus::from_id(id).map(TicketStatus::id), Some(*id));
        }
        for (id, _) in SourceFilter::OPTIONS {
            assert_eq!(SourceFilter::from_id(id).map(SourceFilter::id), Some(*id));
        }
        assert_eq!(Filter::from_id("???"), None);
        assert_eq!(SourceFilter::from_id("???"), None);
    }

    #[test]
    fn filter_matches_scope() {
        assert!(Filter::All.matches(Scope::Mine) && Filter::All.matches(Scope::Team));
        assert!(Filter::Mine.matches(Scope::Mine) && !Filter::Mine.matches(Scope::Team));
        assert!(Filter::Team.matches(Scope::Team) && !Filter::Team.matches(Scope::Mine));
    }

    #[test]
    fn source_filter_matches_source() {
        assert!(
            SourceFilter::All.matches(Source::Jira) && SourceFilter::All.matches(Source::GitHub)
        );
        assert!(
            SourceFilter::Jira.matches(Source::Jira) && !SourceFilter::Jira.matches(Source::GitHub)
        );
        assert!(
            SourceFilter::GitHub.matches(Source::GitHub)
                && !SourceFilter::GitHub.matches(Source::Jira)
        );
    }

    #[test]
    fn priorities_roundtrip_and_default_is_medium() {
        for (id, label) in Priority::OPTIONS {
            let p = Priority::from_id(id).unwrap();
            assert_eq!((p.id(), p.label()), (*id, *label));
        }
        assert_eq!(Priority::from_id("???"), None);
        assert_eq!(Priority::default(), Priority::Medium);
        assert_eq!(Priority::OPTIONS.len(), Priority::ALL.len());
    }

    #[test]
    fn pr_state_suggests_ticket_status() {
        let mut pr = PrRef {
            number: 1,
            draft: true,
            checks: Checks::Passed,
            changes_requested: false,
            merged: false,
            review_requested: false,
        };
        assert_eq!(pr.target_status(), TicketStatus::InProgress);
        pr.draft = false;
        assert_eq!(pr.target_status(), TicketStatus::InProgress, "offen");
        pr.review_requested = true;
        assert_eq!(pr.target_status(), TicketStatus::Review);
        pr.merged = true;
        assert_eq!(pr.target_status(), TicketStatus::Done);
    }

    #[test]
    fn pr_labels() {
        let pr = PrRef {
            number: 87,
            draft: false,
            checks: Checks::Passed,
            changes_requested: false,
            merged: false,
            review_requested: false,
        };
        assert_eq!(pr.label(), "#87 · offen");
        assert_eq!(pr.status().1, Tone::Orange);
        let failed = PrRef {
            number: 85,
            draft: true,
            checks: Checks::Failed,
            changes_requested: false,
            merged: false,
            review_requested: false,
        };
        assert_eq!(failed.label(), "#85 · Entwurf");
        assert_eq!(
            failed.status(),
            ("Checks fehlgeschlagen".to_string(), Tone::Red)
        );
        let merged = PrRef { merged: true, ..pr };
        assert_eq!(merged.label(), "#87 · gemergt");
        assert_eq!(merged.status(), ("Gemergt".to_string(), Tone::Indigo));
    }
}
