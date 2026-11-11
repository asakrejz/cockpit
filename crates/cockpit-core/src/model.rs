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
}

impl Filter {
    /// ID und Beschriftung, in Anzeigereihenfolge.
    pub const OPTIONS: &'static [(&'static str, &'static str)] =
        &[("alle", "Alle"), ("ich", "Ich"), ("team", "Team")];

    pub fn id(self) -> &'static str {
        match self {
            Filter::All => "alle",
            Filter::Mine => "ich",
            Filter::Team => "team",
        }
    }

    pub fn from_id(id: &str) -> Option<Filter> {
        [Filter::All, Filter::Mine, Filter::Team]
            .into_iter()
            .find(|f| f.id() == id)
    }

    pub fn matches(self, scope: Scope) -> bool {
        match self {
            Filter::All => true,
            Filter::Mine => scope == Scope::Mine,
            Filter::Team => scope == Scope::Team,
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
    ReviewRequested,
    AgentFinished,
    ChecksFailed,
    Assigned,
}

impl ReasonKind {
    pub fn tone(self) -> Tone {
        match self {
            ReasonKind::Comment => Tone::Blue,
            ReasonKind::ReviewRequested => Tone::Orange,
            ReasonKind::AgentFinished => Tone::Green,
            ReasonKind::ChecksFailed => Tone::Red,
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
}

impl PrRef {
    /// `#87 · offen`
    pub fn label(&self) -> String {
        format!(
            "#{} · {}",
            self.number,
            if self.draft { "Entwurf" } else { "offen" }
        )
    }

    /// Statuszeile für Pull Requests, bei denen ein Review ansteht.
    pub fn status(&self) -> (String, Tone) {
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
}

impl Item {
    pub fn is_ticket(&self) -> bool {
        self.kind == Kind::Ticket
    }
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
        assert_eq!(Filter::from_id("???"), None);
    }

    #[test]
    fn filter_matches_scope() {
        assert!(Filter::All.matches(Scope::Mine) && Filter::All.matches(Scope::Team));
        assert!(Filter::Mine.matches(Scope::Mine) && !Filter::Mine.matches(Scope::Team));
        assert!(Filter::Team.matches(Scope::Team) && !Filter::Team.matches(Scope::Mine));
    }

    #[test]
    fn pr_labels() {
        let pr = PrRef {
            number: 87,
            draft: false,
            checks: Checks::Passed,
        };
        assert_eq!(pr.label(), "#87 · offen");
        assert_eq!(pr.status().1, Tone::Orange);
        let failed = PrRef {
            number: 85,
            draft: true,
            checks: Checks::Failed,
        };
        assert_eq!(failed.label(), "#85 · Entwurf");
        assert_eq!(
            failed.status(),
            ("Checks fehlgeschlagen".to_string(), Tone::Red)
        );
    }
}
