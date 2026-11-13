//! Data model: inbox entries and what belongs to them.

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

/// Does the entry belong to me or to someone on the team?
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Mine,
    Team,
}

/// Inbox filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Mine,
    Team,
    /// Snoozed entries (see [`crate::attention`]).
    Snoozed,
}

impl Filter {
    /// ID and label, in display order.
    pub const OPTIONS: &'static [(&'static str, &'static str)] = &[
        ("all", "Alle"),
        ("mine", "Ich"),
        ("team", "Team"),
        ("snoozed", "Zurückgestellt"),
    ];

    pub fn id(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Mine => "mine",
            Filter::Team => "team",
            Filter::Snoozed => "snoozed",
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

/// Inbox filter by source: all, only Jira or only GitHub.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceFilter {
    #[default]
    All,
    Jira,
    GitHub,
}

impl SourceFilter {
    /// ID and label, in display order.
    pub const OPTIONS: &'static [(&'static str, &'static str)] =
        &[("all", "Alle"), ("jira", "Jira"), ("github", "GitHub")];

    pub fn id(self) -> &'static str {
        match self {
            SourceFilter::All => "all",
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

/// How important a ticket is. Old states without a value count as „Mittel“.
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

    /// ID and label, in display order (most important first).
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

/// Hue for status dots and icons.
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

/// Why an entry needs attention.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasonKind {
    Comment,
    /// Someone mentioned me in a comment with `@Name`.
    Mention,
    ReviewRequested,
    AgentFinished,
    ChecksFailed,
    /// Changes were requested for my pull request.
    ChangesRequested,
    /// The due date of a ticket lies in the past.
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

    /// The signed-in user.
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
    /// Written by me.
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

/// Identity of a pull request: the repo (`owner/name`) plus the number. Equal numbers
/// from two repos are two different pull requests.
///
/// On the wire a key without repo is a plain number (the format before several repos were
/// supported) and a key with repo is the text `owner/name#12`; both are read. A key without
/// repo matches any repo with that number (old data, single-repo tests); a key with a repo
/// matches only that repo (or an entry whose repo is still unknown).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrKey {
    /// `owner/name`; empty = unknown (old data).
    pub repo: String,
    pub number: u32,
}

impl PrKey {
    pub fn new(repo: &str, number: u32) -> Self {
        Self {
            repo: repo.into(),
            number,
        }
    }

    /// Does this key address the pull request `number` of `repo`? An empty repo on either
    /// side matches (see the type documentation).
    pub fn matches(&self, repo: &str, number: u32) -> bool {
        self.number == number && (self.repo.is_empty() || repo.is_empty() || self.repo == repo)
    }

    /// Does `other` address the same pull request (same rule as [`PrKey::matches`])?
    pub fn same(&self, other: &PrKey) -> bool {
        self.matches(&other.repo, other.number)
    }

    /// Reads `owner/name#12`, `#12` or `12`.
    ///
    /// ```
    /// use cockpit_core::PrKey;
    /// assert_eq!(PrKey::parse("acme/app#12"), Some(PrKey::new("acme/app", 12)));
    /// assert_eq!(PrKey::parse("#12"), Some(PrKey::new("", 12)));
    /// assert_eq!(PrKey::parse("x"), None);
    /// ```
    pub fn parse(text: &str) -> Option<PrKey> {
        let text = text.trim();
        let (repo, number) = text.rsplit_once('#').unwrap_or(("", text));
        let number = number.parse().ok()?;
        if !repo.is_empty() && !repo_name_valid(repo) {
            return None;
        }
        Some(PrKey::new(repo, number))
    }

    /// Part of a hash address: `acme/app/12`, or `12` without repo.
    pub fn route(&self) -> String {
        if self.repo.is_empty() {
            self.number.to_string()
        } else {
            format!("{}/{}", self.repo, self.number)
        }
    }

    /// Reads [`PrKey::route`]; a malformed text gives `None`.
    pub fn from_route(text: &str) -> Option<PrKey> {
        let text = text.trim_matches('/');
        match text.rsplit_once('/') {
            Some((repo, number)) if repo_name_valid(repo) => {
                Some(PrKey::new(repo, number.parse().ok()?))
            }
            Some(_) => None,
            None => Some(PrKey::new("", text.parse().ok()?)),
        }
    }

    /// For names of runs, branches and folders: `acme~app-12`, or `12` without repo. `~`
    /// cannot occur in a repo name, so two repos never give the same text.
    pub fn slug(&self) -> String {
        if self.repo.is_empty() {
            self.number.to_string()
        } else {
            format!("{}-{}", self.repo.replace('/', "~"), self.number)
        }
    }
}

impl std::fmt::Display for PrKey {
    /// `#12`, with repo `acme/app#12`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.repo.is_empty() {
            write!(f, "#{}", self.number)
        } else {
            write!(f, "{}#{}", self.repo, self.number)
        }
    }
}

impl From<u32> for PrKey {
    fn from(number: u32) -> Self {
        PrKey::new("", number)
    }
}

impl From<&PrKey> for PrKey {
    fn from(key: &PrKey) -> Self {
        key.clone()
    }
}

impl Serialize for PrKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.repo.is_empty() {
            s.serialize_u32(self.number)
        } else {
            s.collect_str(self)
        }
    }
}

impl<'de> Deserialize<'de> for PrKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Number(u32),
            Text(String),
            Parts {
                #[serde(default)]
                repo: String,
                number: u32,
            },
        }
        match Wire::deserialize(d)? {
            Wire::Number(number) => Ok(PrKey::new("", number)),
            Wire::Parts { repo, number } => Ok(PrKey::new(&repo, number)),
            Wire::Text(text) => PrKey::parse(&text)
                .ok_or_else(|| serde::de::Error::custom(format!("not a pull request: {text}"))),
        }
    }
}

/// `owner/name` of letters, digits, `.`, `-` and `_`.
pub fn repo_name_valid(repo: &str) -> bool {
    let part = |t: &str| {
        !t.is_empty()
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    repo.split_once('/')
        .is_some_and(|(o, n)| part(o) && part(n))
}

/// Reference to a pull request, as it hangs on an entry.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrRef {
    #[serde(default)]
    pub number: u32,
    /// `owner/name`; empty = unknown (old data, filled by the migration).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    #[serde(default)]
    pub draft: bool,
    pub checks: Checks,
    /// A reviewer requested changes.
    #[serde(default)]
    pub changes_requested: bool,
    /// Already merged.
    #[serde(default)]
    pub merged: bool,
    /// A review is requested.
    #[serde(default)]
    pub review_requested: bool,
}

impl PrRef {
    /// Which ticket status this pull request suggests: merged → Done, review
    /// requested → Review, otherwise (draft or open) → In progress.
    pub fn target_status(&self) -> TicketStatus {
        if self.merged {
            TicketStatus::Done
        } else if self.review_requested {
            TicketStatus::Review
        } else {
            TicketStatus::InProgress
        }
    }

    pub fn key(&self) -> PrKey {
        PrKey::new(&self.repo, self.number)
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

    /// Status line for pull requests with a review pending.
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

/// An inbox entry: a Jira ticket or a GitHub pull request.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Internal ID, e.g. `bau142`.
    #[serde(default)]
    pub id: String,
    pub kind: Kind,
    pub source: Source,
    /// Visible key: `BAU-142` or `#87`.
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
    /// Only relevant for tickets.
    #[serde(default)]
    pub status: TicketStatus,
    /// Only relevant for tickets.
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub pr: Option<PrRef>,
    /// Note from the agent, e.g. „Vom Agenten umgesetzt · Tests bestanden“.
    #[serde(default)]
    pub agent_note: Option<String>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// Done – needs no attention any more.
    #[serde(default)]
    pub read: bool,
    /// Due date as a day (`2026-11-09`), for tickets only.
    #[serde(default)]
    pub due: Option<String>,
    /// The repo (`owner/name`) of a pull request; empty for tickets.
    #[serde(default)]
    pub repo: Option<String>,
    /// Labels of the service (Jira labels, GitHub labels).
    #[serde(default)]
    pub labels: Vec<String>,
}

impl Item {
    pub fn is_ticket(&self) -> bool {
        self.kind == Kind::Ticket
    }

    /// The Jira project of a ticket: the part of the key before the hyphen
    /// (`BAU-142` → `BAU`). Pull requests and other entries have none.
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

    /// Does a run belong to this entry? A run is named after the ticket key; a follow-up job
    /// on a pull request without ticket after the pull request including its repo
    /// (`acme/app#87`), because `#87` alone could be any repo's.
    pub fn matches_run_key(&self, run_key: &str) -> bool {
        self.key == run_key
            || (self.kind == Kind::PullRequest
                && self.pr.as_ref().is_some_and(|p| {
                    PrKey::parse(run_key).is_some_and(|k| !k.repo.is_empty() && k.same(&p.key()))
                }))
    }

    /// The status for filters: for tickets their own, for pull requests the one the pull
    /// request suggests (merged → Done, review requested → Review, otherwise In progress).
    pub fn effective_status(&self) -> TicketStatus {
        match (&self.kind, &self.pr) {
            (Kind::PullRequest, Some(pr)) => pr.target_status(),
            _ => self.status,
        }
    }
}

/// Is this a valid Jira project key? A letter first, then letters, digits
/// or `_`. This way nothing slips into a JQL query.
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
            repo: String::new(),
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
    fn pr_keys_read_and_write_the_old_and_the_new_wire_form() {
        // Old form: a plain number; new form: `owner/name#12`.
        assert_eq!(serde_json::to_string(&PrKey::from(87)).unwrap(), "87");
        let key = PrKey::new("acme/app", 87);
        assert_eq!(serde_json::to_string(&key).unwrap(), "\"acme/app#87\"");
        assert_eq!(
            serde_json::from_str::<PrKey>("87").unwrap(),
            PrKey::from(87)
        );
        assert_eq!(
            serde_json::from_str::<PrKey>("\"acme/app#87\"").unwrap(),
            key
        );
        assert_eq!(
            serde_json::from_str::<PrKey>(r#"{"repo":"acme/app","number":87}"#).unwrap(),
            key
        );
        assert!(serde_json::from_str::<PrKey>("\"nonsense\"").is_err());
    }

    #[test]
    fn pr_keys_match_by_repo_unless_a_repo_is_unknown() {
        let app = PrKey::new("acme/app", 5);
        assert!(app.matches("acme/app", 5));
        assert!(!app.matches("acme/web", 5));
        assert!(!app.matches("acme/app", 6));
        assert!(app.matches("", 5), "an entry of an old state without repo");
        assert!(
            PrKey::from(5).matches("acme/web", 5),
            "a plain number from an old client"
        );
        assert_eq!(app.to_string(), "acme/app#5");
        assert_eq!(PrKey::from(5).to_string(), "#5");
    }

    #[test]
    fn pr_keys_make_hash_addresses_and_safe_names() {
        let key = PrKey::new("acme/app", 5);
        assert_eq!(key.route(), "acme/app/5");
        assert_eq!(PrKey::from_route("acme/app/5"), Some(key.clone()));
        assert_eq!(PrKey::from_route("5"), Some(PrKey::from(5)));
        assert_eq!(PrKey::from_route("abc"), None);
        assert_eq!(PrKey::from_route("x/5"), None, "not owner/name");
        assert_eq!(key.slug(), "acme~app-5");
        assert_ne!(PrKey::new("a-b/c", 1).slug(), PrKey::new("a/b-c", 1).slug());
    }

    #[test]
    fn a_run_belongs_to_the_pull_request_entry_it_names() {
        let mut item = crate::demo::workspace().item_by_key("#87").unwrap().clone();
        let repo = item.pr.as_ref().unwrap().repo.clone();
        assert!(item.matches_run_key("#87"));
        assert!(item.matches_run_key(&format!("{repo}#87")));
        assert!(!item.matches_run_key("other/repo#87"));
        item.pr.as_mut().unwrap().number = 9;
        assert!(!item.matches_run_key(&format!("{repo}#87")));
    }

    #[test]
    fn pr_labels() {
        let pr = PrRef {
            repo: String::new(),
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
            repo: String::new(),
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
