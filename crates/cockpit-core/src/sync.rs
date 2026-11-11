//! Abgleich mit Jira und GitHub: Was die Dienste melden, fließt in den Workspace.
//!
//! Alles hier ist rein – ohne Netz, ohne Uhr. Das Backend holt die Daten
//! (`cockpit-integrations`), packt sie in ein [`Remote`] und wendet sie als Aktion
//! [`Action::SyncRemote`](crate::Action::SyncRemote) an. [`merge`] führt beides zusammen:
//!
//! * **Neue** Einträge kommen dazu (vorn), **entfernte** verschwinden.
//! * **Lokaler Zustand bleibt**: gelesen/erledigt, Zurückstellungen, Agent-Läufe und
//!   Hinweise des Agenten, Vorschläge und Entscheidungen im Review, eigene noch nicht
//!   gesendete Zeilenkommentare, Urteil und „gesendet“.
//! * **Von außen Geänderte** (neue Kommentare, Status, Checks, angeforderte Änderungen)
//!   erscheinen als [`Change`] – das Protokoll bekommt dafür Einträge mit Urheber `System`,
//!   und `notify::events` sieht sie im Vorher/Nachher wie jede andere Änderung.
//! * Scheitert eine Quelle (Token abgelaufen, Netz weg, Kontingent), bleiben ihre Daten
//!   unverändert; der Zustand je Quelle ([`SourceState`]) hält fest, woran es liegt.
//!
//! Beim ersten erfolgreichen Abgleich verschwinden die Beispieldaten (nur die
//! unveränderten Teile der Demo), damit echte Daten nicht mit ihnen vermischt werden.

use serde::{Deserialize, Serialize};

use crate::action::Target;
use crate::agent::AgentRun;
use crate::demo;
use crate::model::{Checks, Item, Source, TicketStatus};
use crate::review::{FileDiff, LineComment, Review};
use crate::workspace::Workspace;

/// So viele neue oder entfernte Einträge stehen einzeln im Protokoll; darüber eine Sammelzeile.
const MAX_EINZELN: usize = 5;
/// So viele Hinweise je Quelle behält der Zustand.
const MAX_WARNUNGEN: usize = 5;

// ---------- Was die Dienste liefern ----------

/// Der Stand beider Dienste nach einem Abruf. Eine Quelle, die fehlt, wurde nicht abgefragt
/// (nicht konfiguriert, kein Schlüssel, Pause wegen Kontingent) – ihre Daten bleiben.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Remote {
    #[serde(default)]
    pub sources: Vec<SourceData>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SourceData {
    pub source: Source,
    pub outcome: Outcome,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Der vollständige Stand der Quelle. Was hier fehlt, ist dort verschwunden.
    Loaded(Loaded),
    /// Der Abruf ist gescheitert.
    Failed(Problem),
}

/// Der vollständige Stand einer Quelle.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    #[serde(default)]
    pub items: Vec<Item>,
    /// Nur GitHub: die Pull Requests als Review (Diff, Checks).
    #[serde(default)]
    pub reviews: Vec<Review>,
    /// Nur GitHub: zu welchem Repo welche Nummer gehört.
    #[serde(default)]
    pub prs: Vec<PrLink>,
    /// Hinweise für den Zustand der Quelle (z. B. ausgelassene Pull Requests).
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Zu welchem Repo ein Pull Request gehört. Das Modell kennt Reviews nur über die Nummer;
/// für Schreibzugriffe braucht das Backend das Repo.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrLink {
    pub number: u32,
    /// `owner/name`
    pub repo: String,
    /// Letzter bekannter Commit des Branches (spart beim nächsten Abruf den Diff).
    #[serde(default)]
    pub head_sha: String,
}

// ---------- Zustand je Quelle ----------

/// Woran ein Abruf gescheitert ist.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// 401: Token fehlt, ist abgelaufen oder widerrufen.
    Unauthorized,
    /// 403: Token gültig, aber ohne Berechtigung.
    NoPermission,
    /// Kontingent aufgebraucht; bis dann (Millisekunden seit 1970) wird nicht abgefragt.
    RateLimited { until_ms: Option<u64> },
    /// Keine Verbindung zum Dienst.
    Offline,
    /// Alles andere, mit deutscher Meldung (ohne Schlüssel).
    Other(String),
}

impl Problem {
    pub fn text(&self) -> String {
        match self {
            Problem::Unauthorized => {
                "Token abgelaufen oder ungültig – neu setzen mit „cockpit-server token setzen“"
                    .into()
            }
            Problem::NoPermission => "Dem Token fehlt eine Berechtigung".into(),
            Problem::RateLimited { until_ms: Some(_) } => {
                "Kontingent aufgebraucht – der Abgleich pausiert".into()
            }
            Problem::RateLimited { until_ms: None } => "Kontingent aufgebraucht".into(),
            Problem::Offline => "Keine Verbindung zum Dienst".into(),
            Problem::Other(m) => m.clone(),
        }
    }
}

/// Wie es um den Abgleich einer Quelle steht. Fehlt die Quelle in der Liste, ist sie aus.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SourceState {
    pub source: Source,
    /// `None` = der letzte Abruf war erfolgreich.
    #[serde(default)]
    pub problem: Option<Problem>,
    /// Letzter erfolgreicher Abgleich (Millisekunden seit 1970).
    #[serde(default)]
    pub last_ok_ms: Option<u64>,
    /// Letzter Versuch.
    #[serde(default)]
    pub last_try_ms: u64,
    /// Hinweise vom letzten erfolgreichen Abruf.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl SourceState {
    pub fn is_ok(&self) -> bool {
        self.problem.is_none()
    }

    /// Kurze Zustandsangabe für die Oberfläche und `/api/health`.
    pub fn id(&self) -> &'static str {
        match &self.problem {
            None => "ok",
            Some(Problem::Unauthorized) => "nicht-angemeldet",
            Some(Problem::NoPermission) => "keine-berechtigung",
            Some(Problem::RateLimited { .. }) => "kontingent",
            Some(Problem::Offline) => "offline",
            Some(Problem::Other(_)) => "fehler",
        }
    }
}

/// Alles, was der Abgleich im Workspace festhält.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncState {
    /// Zustand je abgefragter Quelle.
    #[serde(default)]
    pub sources: Vec<SourceState>,
    /// IDs der Einträge, die von einem Dienst kommen. Nur für sie gehen Aktionen nach außen.
    #[serde(default)]
    pub items: Vec<String>,
    /// Pull Requests, die von GitHub kommen.
    #[serde(default)]
    pub prs: Vec<PrLink>,
    /// Die Beispieldaten sind schon entfernt.
    #[serde(default)]
    pub demo_dropped: bool,
}

impl SyncState {
    pub fn state_of(&self, source: Source) -> Option<&SourceState> {
        self.sources.iter().find(|s| s.source == source)
    }

    /// Kommt dieser Eintrag von einem Dienst (und nicht aus den Beispieldaten)?
    pub fn is_synced(&self, item_id: &str) -> bool {
        self.items.iter().any(|i| i == item_id)
    }

    pub fn pr(&self, number: u32) -> Option<&PrLink> {
        self.prs.iter().find(|p| p.number == number)
    }

    /// Bis wann pausiert der Abgleich dieser Quelle wegen des Kontingents?
    pub fn paused_until(&self, source: Source, now_ms: u64) -> Option<u64> {
        match self.state_of(source)?.problem {
            Some(Problem::RateLimited { until_ms: Some(t) }) if t > now_ms => Some(t),
            _ => None,
        }
    }

    fn set_state(
        &mut self,
        source: Source,
        problem: Option<Problem>,
        now_ms: u64,
        warn: &[String],
    ) {
        let entry = match self.sources.iter().position(|s| s.source == source) {
            Some(i) => &mut self.sources[i],
            None => {
                self.sources.push(SourceState {
                    source,
                    problem: None,
                    last_ok_ms: None,
                    last_try_ms: now_ms,
                    warnings: Vec::new(),
                });
                self.sources.last_mut().expect("gerade eingefügt")
            }
        };
        entry.last_try_ms = now_ms;
        if problem.is_none() {
            entry.last_ok_ms = Some(now_ms);
            entry.warnings = warn.iter().take(MAX_WARNUNGEN).cloned().collect();
        }
        entry.problem = problem;
    }
}

// ---------- Änderungen ----------

/// Was sich beim Abgleich von außen geändert hat. Daraus entstehen die Protokolleinträge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    ItemAdded {
        id: String,
        key: String,
        title: String,
    },
    ItemRemoved {
        id: String,
        key: String,
    },
    NewComment {
        id: String,
        key: String,
        author: String,
    },
    StatusChanged {
        id: String,
        key: String,
        to: TicketStatus,
    },
    ChecksChanged {
        id: String,
        number: u32,
        to: Checks,
    },
    ChangesRequested {
        id: String,
        number: u32,
    },
    /// Ein eigener, noch nicht gesendeter Kommentar passt nach neuen Commits nicht mehr zum Diff.
    DraftDropped {
        number: u32,
        place: String,
        text: String,
    },
    SourceProblem {
        source: Source,
        problem: Problem,
    },
    SourceRecovered {
        source: Source,
    },
    /// Sammelzeile, wenn viele Einträge auf einmal kommen oder gehen.
    Summary(String),
}

impl Change {
    /// Kurzer deutscher Satz fürs Protokoll.
    pub fn text(&self) -> String {
        match self {
            Change::ItemAdded { key, title, .. } => format!("{key} neu: {title}"),
            Change::ItemRemoved { key, .. } => {
                format!("{key} ist nicht mehr dabei (erledigt oder entfernt)")
            }
            Change::NewComment { key, author, .. } => format!("{author} hat {key} kommentiert"),
            Change::StatusChanged { key, to, .. } => {
                format!(
                    "Status von {key} ist jetzt „{}“ (von außen geändert)",
                    label(*to)
                )
            }
            Change::ChecksChanged { number, to, .. } => {
                format!("Checks für #{number}: {}", to.label())
            }
            Change::ChangesRequested { number, .. } => {
                format!("Änderungen angefordert in #{number}")
            }
            Change::DraftDropped {
                number,
                place,
                text,
            } => format!(
                "Dein Entwurf an {place} in #{number} passt nicht mehr zum geänderten Diff und entfällt: „{text}“"
            ),
            Change::SourceProblem { source, problem } => {
                format!("{}: {}", source.label(), problem.text())
            }
            Change::SourceRecovered { source } => format!("{}: wieder verbunden", source.label()),
            Change::Summary(text) => text.clone(),
        }
    }

    pub fn target(&self) -> Option<Target> {
        match self {
            Change::ItemAdded { id, .. }
            | Change::ItemRemoved { id, .. }
            | Change::NewComment { id, .. }
            | Change::StatusChanged { id, .. }
            | Change::ChecksChanged { id, .. }
            | Change::ChangesRequested { id, .. } => Some(Target::Item(id.clone())),
            Change::DraftDropped { number, .. } => Some(Target::Review(*number)),
            Change::SourceProblem { .. } | Change::SourceRecovered { .. } | Change::Summary(_) => {
                None
            }
        }
    }
}

fn label(status: TicketStatus) -> &'static str {
    TicketStatus::OPTIONS
        .iter()
        .find(|(id, _)| *id == status.id())
        .map_or("", |(_, l)| l)
}

// ---------- Zusammenführen ----------

/// Führt den Stand der Dienste mit dem lokalen Workspace zusammen. Rein: `local` bleibt
/// unverändert, `now_ms` kommt von außen.
pub fn merge(local: &Workspace, remote: &Remote, now_ms: u64) -> (Workspace, Vec<Change>) {
    let mut ws = local.clone();
    let mut changes = Vec::new();

    for data in &remote.sources {
        let source = data.source;
        let before = ws.sync.state_of(source).and_then(|s| s.problem.clone());
        match &data.outcome {
            Outcome::Failed(problem) => {
                ws.sync
                    .set_state(source, Some(problem.clone()), now_ms, &[]);
                if before.as_ref() != Some(problem) {
                    changes.push(Change::SourceProblem {
                        source,
                        problem: problem.clone(),
                    });
                }
            }
            Outcome::Loaded(loaded) => {
                if !ws.sync.demo_dropped {
                    drop_demo(&mut ws);
                    ws.sync.demo_dropped = true;
                }
                merge_source(&mut ws, source, loaded, &mut changes);
                ws.sync.set_state(source, None, now_ms, &loaded.warnings);
                if before.is_some() {
                    changes.push(Change::SourceRecovered { source });
                }
            }
        }
    }
    // Zurückstellungen von Einträgen, die es nicht mehr gibt, räumen wir weg.
    let items = &ws.items;
    ws.snoozed.retain(|s| items.iter().any(|i| i.id == s.item));
    (ws, changes)
}

/// Entfernt, was noch genau so aus den Beispieldaten stammt.
fn drop_demo(ws: &mut Workspace) {
    let demo = demo::workspace();
    ws.items
        .retain(|i| !demo.items.iter().any(|d| d.id == i.id) || ws.sync.is_synced(&i.id));
    ws.reviews.retain(|r| {
        ws.sync.pr(r.number).is_some() || !demo.reviews.iter().any(|d| d.number == r.number)
    });
    ws.runs
        .retain(|r| !demo.runs.iter().any(|d: &AgentRun| d == r));
}

fn merge_source(ws: &mut Workspace, source: Source, loaded: &Loaded, changes: &mut Vec<Change>) {
    merge_items(ws, source, &loaded.items, changes);
    if source == Source::GitHub {
        merge_reviews(ws, loaded, changes);
    }
}

fn merge_items(ws: &mut Workspace, source: Source, remote: &[Item], changes: &mut Vec<Change>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();

    // Bestehende Einträge dieser Quelle: aktualisieren oder entfernen.
    let old = std::mem::take(&mut ws.items);
    let mut kept = Vec::with_capacity(old.len());
    for item in old {
        if item.source != source || !ws.sync.is_synced(&item.id) {
            kept.push(item);
            continue;
        }
        match remote.iter().find(|r| r.id == item.id) {
            Some(r) => kept.push(merge_item(item, r, changes)),
            None => removed.push(Change::ItemRemoved {
                id: item.id,
                key: item.key,
            }),
        }
    }
    // Neue Einträge kommen nach vorn, in der Reihenfolge des Dienstes.
    let mut fresh = Vec::new();
    for r in remote {
        if !kept.iter().any(|i| i.id == r.id) && !fresh.iter().any(|i: &Item| i.id == r.id) {
            added.push(Change::ItemAdded {
                id: r.id.clone(),
                key: r.key.clone(),
                title: r.title.clone(),
            });
            fresh.push(r.clone());
        }
    }
    fresh.append(&mut kept);
    ws.items = fresh;

    // Die Liste der Einträge, die von Diensten kommen.
    ws.sync
        .items
        .retain(|id| ws.items.iter().any(|i| &i.id == id));
    for r in remote {
        if !ws.sync.is_synced(&r.id) {
            ws.sync.items.push(r.id.clone());
        }
    }
    collapse(added, changes, "neue Einträge");
    collapse(removed, changes, "Einträge nicht mehr dabei");
}

/// Bis [`MAX_EINZELN`] einzeln, darüber eine Sammelzeile (die erste Änderung als Träger).
fn collapse(list: Vec<Change>, out: &mut Vec<Change>, noun: &str) {
    if list.len() <= MAX_EINZELN {
        out.extend(list);
        return;
    }
    out.push(Change::Summary(format!("{} {noun}", list.len())));
}

/// Der lokale Eintrag `old` bekommt die Werte des Dienstes; lokaler Zustand bleibt.
fn merge_item(old: Item, remote: &Item, changes: &mut Vec<Change>) -> Item {
    let mut item = remote.clone();

    // Lokal bleibt: gelesen, Hinweis des Agenten, Fälligkeit ohne Gegenstück im Dienst,
    // Branch bei Tickets (Jira kennt ihn nicht).
    item.read = old.read;
    item.agent_note = old.agent_note.clone();
    if item.due.is_none() && remote.source == Source::GitHub {
        item.due = old.due.clone();
    }
    if item.branch.is_none() {
        item.branch = old.branch.clone();
    }
    // Priorität und Verknüpfung mit dem Pull Request pflegt Cockpit (Jira liefert beides heute
    // nicht mit): Was du gesetzt hast, bleibt, bis der Dienst selbst einen Wert meldet.
    if item.priority == crate::model::Priority::default() {
        item.priority = old.priority;
    }
    if item.is_ticket() && item.pr.is_none() {
        item.pr = old.pr.clone();
    }
    // Das Ergebnis des Agenten ist ein lokaler Grund; er bleibt, bis der Dienst etwas Neues sagt.
    if old.reason == crate::model::ReasonKind::AgentFinished && old.is_ticket() {
        item.reason = old.reason;
        item.reason_text = old.reason_text.clone();
    }

    let mut bump = false;

    // Neue Kommentare von anderen.
    let foreign = |list: &[crate::model::Comment]| list.iter().filter(|c| !c.own).count();
    let seen = foreign(&old.comments);
    let now_foreign: Vec<_> = remote.comments.iter().filter(|c| !c.own).collect();
    if now_foreign.len() > seen {
        for c in &now_foreign[seen..] {
            changes.push(Change::NewComment {
                id: item.id.clone(),
                key: item.key.clone(),
                author: c.author.name.clone(),
            });
        }
        bump = true;
    }

    if item.is_ticket() && item.status != old.status {
        changes.push(Change::StatusChanged {
            id: item.id.clone(),
            key: item.key.clone(),
            to: item.status,
        });
        bump = true;
    }

    if let (Some(new), Some(prev)) = (&item.pr, &old.pr) {
        if new.checks != prev.checks {
            changes.push(Change::ChecksChanged {
                id: item.id.clone(),
                number: new.number,
                to: new.checks,
            });
            bump = true;
        }
        if new.changes_requested && !prev.changes_requested {
            changes.push(Change::ChangesRequested {
                id: item.id.clone(),
                number: new.number,
            });
            bump = true;
        }
    }
    if item.reason != old.reason && item.reason != crate::model::ReasonKind::Comment {
        bump = true;
    }
    if bump {
        item.read = false;
    }
    item
}

fn merge_reviews(ws: &mut Workspace, loaded: &Loaded, changes: &mut Vec<Change>) {
    let old = std::mem::take(&mut ws.reviews);
    let mut kept = Vec::with_capacity(old.len());
    let mut by_number: Vec<Review> = Vec::new();
    for review in old {
        if ws.sync.pr(review.number).is_some() {
            by_number.push(review);
        } else {
            kept.push(review);
        }
    }
    // Reviews, die es auf GitHub nicht mehr gibt, verschwinden (nur verknüpfte).
    let mut merged: Vec<Review> = Vec::new();
    for r in &loaded.reviews {
        let previous = by_number.iter().position(|o| o.number == r.number);
        let review = match previous {
            Some(i) => merge_review(by_number.remove(i), r, changes),
            None => {
                // Nummer schon lokal belegt (Beispiel oder Simulation): der Dienst gewinnt.
                kept.retain(|k| k.number != r.number);
                r.clone()
            }
        };
        merged.push(review);
    }
    // Neue Reviews nach vorn, vorhandene an ihrem Platz wie bisher: einfach vor die übrigen.
    merged.append(&mut kept);
    ws.reviews = merged;

    ws.sync.prs = loaded.prs.clone();
}

/// Das lokale Review `old` bekommt Diff, Titel und Checks des Dienstes.
fn merge_review(old: Review, remote: &Review, changes: &mut Vec<Change>) -> Review {
    let mut review = remote.clone();
    review.agent_reviewed = old.agent_reviewed;
    review.suggestions = old.suggestions.clone();
    review.verdict = old.verdict;
    review.sent = old.sent;
    review.merged = old.merged;
    if review.ticket_key.is_none() {
        review.ticket_key = old.ticket_key.clone();
    }
    review.line_comments = carry_drafts(&old, &review.files, changes);
    review
}

/// Eigene Entwürfe in den neuen Diff übertragen. Die Position ist ein Index in den Zeilen
/// der Datei; ändert sich der Diff, suchen wir dieselbe Zeile (Art, Nummer, Text) neu.
fn carry_drafts(
    old: &Review,
    new_files: &[FileDiff],
    changes: &mut Vec<Change>,
) -> Vec<LineComment> {
    let mut out = Vec::new();
    for draft in &old.line_comments {
        if old.files == new_files {
            out.push(draft.clone());
            continue;
        }
        let old_lines = old
            .files
            .iter()
            .find(|f| f.name == draft.file)
            .map(|f| f.lines.as_slice());
        let line = old_lines.and_then(|l| l.get(draft.line));
        // Dieselbe Zeile (Art und Text; die Nummer verschiebt sich) – bei mehreren gleichen
        // die gleich vielte.
        let moved = line.and_then(|l| {
            let same = |x: &&crate::review::DiffLine| x.kind == l.kind && x.text == l.text;
            let nth = old_lines?[..draft.line].iter().filter(same).count();
            new_files
                .iter()
                .find(|f| f.name == draft.file)?
                .lines
                .iter()
                .enumerate()
                .filter(|(_, x)| same(x))
                .nth(nth)
                .map(|(i, _)| i)
        });
        match moved {
            Some(index) => out.push(LineComment {
                line: index,
                ..draft.clone()
            }),
            None => changes.push(Change::DraftDropped {
                number: old.number,
                place: match line {
                    Some(l) => format!("{}, Zeile {}", draft.file, l.no),
                    None => draft.file.clone(),
                },
                text: draft.text.clone(),
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Action, Actor};
    use crate::model::{Comment, Kind, Person, PrRef, ReasonKind, Scope};
    use crate::review::{Decision, LineKind, Suggestion, Verdict};
    use crate::{notify, persist};

    const T: u64 = 1_800_000_000_000;

    fn ticket(key: &str, title: &str) -> Item {
        Item {
            id: key.to_lowercase(),
            kind: Kind::Ticket,
            source: Source::Jira,
            key: key.into(),
            title: title.into(),
            description: String::new(),
            reason: ReasonKind::Assigned,
            reason_text: "Dir zugewiesen".into(),
            time: "vor 5 Min.".into(),
            scope: Scope::Mine,
            owner: Person::me(),
            status: TicketStatus::Open,
            branch: None,
            pr: None,
            agent_note: None,
            comments: Vec::new(),
            read: false,
            due: None,
            priority: Default::default(),
        }
    }

    fn pr_item(number: u32) -> Item {
        Item {
            id: format!("gh-a-x-{number}"),
            kind: Kind::PullRequest,
            source: Source::GitHub,
            key: format!("#{number}"),
            title: format!("PR {number}"),
            reason: ReasonKind::ReviewRequested,
            reason_text: "Review angefragt".into(),
            pr: Some(PrRef {
                number,
                draft: false,
                checks: Checks::Passed,
                changes_requested: false,
                merged: false,
                review_requested: true,
            }),
            branch: Some("feature/x".into()),
            ..ticket("X-1", "")
        }
    }

    fn files(lines: &[(u32, LineKind, &str)]) -> Vec<FileDiff> {
        vec![FileDiff::new("a.rs", lines)]
    }

    fn review(number: u32, files: Vec<FileDiff>) -> Review {
        Review {
            number,
            title: format!("PR {number}"),
            description: String::new(),
            author: Person::new("Lena", "LK"),
            branch: "feature/x".into(),
            base: "main".into(),
            ticket_key: None,
            checks: Checks::Passed,
            files,
            agent_reviewed: false,
            suggestions: Vec::new(),
            line_comments: Vec::new(),
            verdict: Verdict::default(),
            sent: false,
            check_runs: Vec::new(),
            merged: None,
        }
    }

    fn jira(items: Vec<Item>) -> Remote {
        Remote {
            sources: vec![SourceData {
                source: Source::Jira,
                outcome: Outcome::Loaded(Loaded {
                    items,
                    ..Loaded::default()
                }),
            }],
        }
    }

    fn github(items: Vec<Item>, reviews: Vec<Review>) -> Remote {
        let prs = reviews
            .iter()
            .map(|r| PrLink {
                number: r.number,
                repo: "a/x".into(),
                head_sha: "abc".into(),
            })
            .collect();
        Remote {
            sources: vec![SourceData {
                source: Source::GitHub,
                outcome: Outcome::Loaded(Loaded {
                    items,
                    reviews,
                    prs,
                    warnings: Vec::new(),
                }),
            }],
        }
    }

    fn synced(remote: &Remote) -> Workspace {
        merge(&Workspace::default(), remote, T).0
    }

    #[test]
    fn new_entries_come_in_front_and_the_demo_goes() {
        let (ws, changes) = merge(
            &demo::workspace(),
            &jira(vec![ticket("ABC-1", "Eins"), ticket("ABC-2", "Zwei")]),
            T,
        );
        let keys: Vec<&str> = ws.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["ABC-1", "ABC-2"], "Beispieldaten sind weg");
        assert!(ws.reviews.is_empty() && ws.runs.is_empty());
        assert!(ws.sync.demo_dropped);
        assert!(ws.sync.is_synced("abc-1"));
        assert_eq!(
            changes
                .iter()
                .filter(|c| matches!(c, Change::ItemAdded { .. }))
                .count(),
            2
        );
        // Später neu Dazugekommenes steht vorn, Vorhandenes behält seinen Platz.
        let (ws, _) = merge(
            &ws,
            &jira(vec![
                ticket("ABC-3", "Drei"),
                ticket("ABC-1", "Eins"),
                ticket("ABC-2", "Zwei"),
            ]),
            T + 1,
        );
        let keys: Vec<&str> = ws.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["ABC-3", "ABC-1", "ABC-2"]);
    }

    #[test]
    fn removed_entries_vanish_but_other_sources_stay() {
        let mut ws = synced(&jira(vec![
            ticket("ABC-1", "Eins"),
            ticket("ABC-2", "Zwei"),
        ]));
        ws = merge(&ws, &github(vec![pr_item(5)], vec![review(5, vec![])]), T).0;
        let (ws, changes) = merge(&ws, &jira(vec![ticket("ABC-2", "Zwei")]), T + 1);
        assert!(ws.item("abc-1").is_none());
        assert!(ws.item("abc-2").is_some() && ws.item("gh-a-x-5").is_some());
        assert!(!ws.sync.is_synced("abc-1"));
        assert!(changes.contains(&Change::ItemRemoved {
            id: "abc-1".into(),
            key: "ABC-1".into()
        }));
        assert!(ws.review(5).is_some(), "GitHub ist nicht betroffen");
    }

    #[test]
    fn local_state_survives_a_sync() {
        let lines = [
            (1, LineKind::Context, "fn a() {"),
            (2, LineKind::Add, "    foo();"),
        ];
        let mut ws = synced(&github(vec![pr_item(5)], vec![review(5, files(&lines))]));
        ws = merge(&ws, &jira(vec![ticket("ABC-1", "Eins")]), T).0;
        // Lokale Zustände …
        ws.mark_done("abc-1");
        ws.snooze("abc-1", Some(T + 10_000_000));
        ws.items[1].agent_note = Some("Agent wartet".into());
        ws.runs.push(AgentRun::new("ABC-1", "Eins"));
        let r = ws.reviews.iter_mut().find(|r| r.number == 5).unwrap();
        r.suggestions.push(Suggestion::new(
            "s1",
            "a.rs · Zeile 2",
            crate::Level::Note,
            "Hm",
        ));
        r.decide("s1", Some(Decision::Approved));
        r.set_verdict(Verdict::RequestChanges);
        r.add_line_comment("a.rs", 1, "Bitte prüfen", Some("    bar();"))
            .unwrap();

        // … bleiben bei jedem Abgleich.
        let remote = Remote {
            sources: [
                jira(vec![ticket("ABC-1", "Eins neu betitelt")]).sources,
                github(vec![pr_item(5)], vec![review(5, files(&lines))]).sources,
            ]
            .concat(),
        };
        let (after, _) = merge(&ws, &remote, T + 1);
        let item = after.item("abc-1").unwrap();
        assert_eq!(
            item.title, "Eins neu betitelt",
            "der Dienst bestimmt den Titel"
        );
        assert!(item.read, "erledigt bleibt erledigt");
        assert!(after.snooze_of("abc-1").is_some());
        assert_eq!(after.runs.len(), 1);
        assert_eq!(
            after.item("gh-a-x-5").unwrap().agent_note.as_deref(),
            Some("Agent wartet")
        );
        let r = after.review(5).unwrap();
        assert_eq!(r.suggestions[0].decision, Some(Decision::Approved));
        assert_eq!(r.verdict, Verdict::RequestChanges);
        assert_eq!(r.line_comments.len(), 1);
        assert_eq!(r.line_comments[0].suggestion.as_deref(), Some("    bar();"));
    }

    #[test]
    fn sent_review_and_merge_state_stay() {
        let mut ws = synced(&github(vec![pr_item(5)], vec![review(5, vec![])]));
        let r = ws.reviews.iter_mut().find(|r| r.number == 5).unwrap();
        r.sent = true;
        r.merged = Some(crate::MergeMethod::Squash);
        let (after, _) = merge(
            &ws,
            &github(vec![pr_item(5)], vec![review(5, vec![])]),
            T + 1,
        );
        assert!(after.review(5).unwrap().sent);
        assert!(after.review(5).unwrap().merged.is_some());
    }

    #[test]
    fn external_comment_is_logged_by_the_system_and_notifies() {
        let ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
        let mut ws = ws;
        ws.mark_done("abc-1");
        let mut remote_item = ticket("ABC-1", "Eins");
        remote_item.comments = vec![Comment::new(
            Person::new("Lena", "LK"),
            "gerade eben",
            "Kannst du das ansehen, @Du?",
        )];
        let before = ws.clone();
        let applied = ws
            .apply(
                Action::SyncRemote {
                    remote: jira(vec![remote_item]),
                },
                Actor::System,
                T + 1,
            )
            .unwrap();
        assert_eq!(applied.logged, 1);
        let entry = ws.log.last().unwrap();
        assert_eq!(entry.actor, Actor::System);
        assert_eq!(entry.text, "Lena hat ABC-1 kommentiert");
        assert_eq!(entry.target, Some(Target::Item("abc-1".into())));
        assert!(!ws.item("abc-1").unwrap().read, "neu zu lesen");
        // notify arbeitet auf vorher/nachher und sieht die Erwähnung.
        let events = notify::events(&before, &ws);
        assert!(
            events
                .iter()
                .any(|e| e.reason == notify::NotifyReason::Mention),
            "{events:?}"
        );
    }

    #[test]
    fn status_and_checks_changes_from_outside_are_reported() {
        let mut ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
        ws = merge(&ws, &github(vec![pr_item(5)], vec![review(5, vec![])]), T).0;
        ws.mark_done("abc-1");
        let mut t = ticket("ABC-1", "Eins");
        t.status = TicketStatus::Done;
        let mut pr = pr_item(5);
        pr.pr.as_mut().unwrap().checks = Checks::Failed;
        pr.pr.as_mut().unwrap().changes_requested = true;
        let remote = Remote {
            sources: [
                jira(vec![t]).sources,
                github(vec![pr], vec![review(5, vec![])]).sources,
            ]
            .concat(),
        };
        let (after, changes) = merge(&ws, &remote, T + 1);
        assert!(changes.iter().any(|c| matches!(
            c,
            Change::StatusChanged {
                to: TicketStatus::Done,
                ..
            }
        )));
        assert!(changes.iter().any(|c| matches!(
            c,
            Change::ChecksChanged {
                number: 5,
                to: Checks::Failed,
                ..
            }
        )));
        assert!(
            changes
                .iter()
                .any(|c| matches!(c, Change::ChangesRequested { number: 5, .. }))
        );
        assert!(
            !after.item("abc-1").unwrap().read,
            "Änderung von außen holt zurück"
        );
        // Gleicher Stand noch einmal: keine Änderungen.
        let (_, again) = merge(&after, &remote, T + 2);
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn own_status_change_is_not_reported_back() {
        let mut ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
        ws.set_status("abc-1", TicketStatus::InProgress);
        ws.mark_done("abc-1");
        let mut t = ticket("ABC-1", "Eins");
        t.status = TicketStatus::InProgress;
        let (after, changes) = merge(&ws, &jira(vec![t]), T + 1);
        assert!(changes.is_empty());
        assert!(after.item("abc-1").unwrap().read);
    }

    #[test]
    fn local_priority_and_pr_link_survive_the_sync() {
        let mut local = ticket("X-1", "Titel");
        local.priority = crate::model::Priority::Highest;
        local.pr = pr_item(7).pr;
        let mut changes = Vec::new();
        let merged = merge_item(local, &ticket("X-1", "Neuer Titel"), &mut changes);
        assert_eq!(merged.title, "Neuer Titel");
        assert_eq!(merged.priority, crate::model::Priority::Highest);
        assert_eq!(merged.pr.map(|p| p.number), Some(7));
    }

    #[test]
    fn failed_source_keeps_its_data_and_reports_the_problem() {
        let ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
        let failed = |p: Problem| Remote {
            sources: vec![SourceData {
                source: Source::Jira,
                outcome: Outcome::Failed(p),
            }],
        };
        let (ws, changes) = merge(&ws, &failed(Problem::Unauthorized), T + 5);
        assert!(ws.item("abc-1").is_some(), "nichts geht verloren");
        let state = ws.sync.state_of(Source::Jira).unwrap();
        assert_eq!(state.problem, Some(Problem::Unauthorized));
        assert_eq!(state.id(), "nicht-angemeldet");
        assert_eq!(state.last_ok_ms, Some(T));
        assert_eq!(state.last_try_ms, T + 5);
        assert_eq!(changes.len(), 1);
        // Dasselbe Problem noch einmal ist nicht noch eine Meldung.
        let (ws, changes) = merge(&ws, &failed(Problem::Unauthorized), T + 6);
        assert!(changes.is_empty());
        // Kontingent: Pause bis zum Zeitpunkt.
        let (ws, _) = merge(
            &ws,
            &failed(Problem::RateLimited {
                until_ms: Some(T + 60_000),
            }),
            T + 7,
        );
        assert_eq!(ws.sync.paused_until(Source::Jira, T + 8), Some(T + 60_000));
        assert_eq!(ws.sync.paused_until(Source::Jira, T + 60_001), None);
        assert_eq!(ws.sync.paused_until(Source::GitHub, T), None);
        // Erholt.
        let (ws, changes) = merge(&ws, &jira(vec![ticket("ABC-1", "Eins")]), T + 70_000);
        assert!(ws.sync.state_of(Source::Jira).unwrap().is_ok());
        assert_eq!(
            changes,
            vec![Change::SourceRecovered {
                source: Source::Jira
            }]
        );
    }

    #[test]
    fn drafts_follow_their_line_when_the_diff_shifts() {
        let old = [
            (1, LineKind::Context, "fn a() {"),
            (2, LineKind::Add, "    foo();"),
            (3, LineKind::Context, "}"),
        ];
        let mut ws = synced(&github(vec![pr_item(5)], vec![review(5, files(&old))]));
        let id = ws.add_line_comment(5, "a.rs", 1, "Warum?", None).unwrap();
        // Neuer Commit fügt vorn eine Zeile ein.
        let new = [
            (1, LineKind::Add, "use x;"),
            (2, LineKind::Context, "fn a() {"),
            (3, LineKind::Add, "    foo();"),
            (4, LineKind::Context, "}"),
        ];
        let (after, changes) = merge(
            &ws,
            &github(vec![pr_item(5)], vec![review(5, files(&new))]),
            T + 1,
        );
        let c = &after.review(5).unwrap().line_comments[0];
        assert_eq!((c.id.as_str(), c.line), (id.as_str(), 2));
        assert!(changes.is_empty());

        // Die Zeile gibt es nicht mehr: Entwurf entfällt, der Text steht im Protokoll.
        let gone = [(1, LineKind::Context, "fn b() {")];
        let (after, changes) = merge(
            &after,
            &github(vec![pr_item(5)], vec![review(5, files(&gone))]),
            T + 2,
        );
        assert!(after.review(5).unwrap().line_comments.is_empty());
        assert!(changes[0].text().contains("Warum?"), "{changes:?}");
    }

    #[test]
    fn a_review_number_from_the_demo_or_simulation_does_not_leak_into_real_state() {
        let mut ws = Workspace::default();
        ws.reviews.push(review(5, vec![]));
        ws.reviews[0].verdict = Verdict::Approve;
        ws.reviews[0].sent = true;
        let (after, _) = merge(&ws, &github(vec![pr_item(5)], vec![review(5, vec![])]), T);
        assert_eq!(after.review(5).unwrap().verdict, Verdict::default());
        assert!(!after.review(5).unwrap().sent);
        assert_eq!(after.sync.pr(5).unwrap().repo, "a/x");
    }

    #[test]
    fn many_new_entries_become_one_log_line() {
        let many: Vec<Item> = (1..=8).map(|n| ticket(&format!("ABC-{n}"), "t")).collect();
        let (_, changes) = merge(&Workspace::default(), &jira(many), T);
        assert_eq!(changes, vec![Change::Summary("8 neue Einträge".into())]);
    }

    #[test]
    fn only_the_system_may_apply_a_sync() {
        let mut ws = Workspace::default();
        let action = Action::SyncRemote {
            remote: Remote::default(),
        };
        assert_eq!(
            ws.apply(action.clone(), Actor::Du, T),
            Err(crate::ActionError::SystemOnly)
        );
        assert!(ws.apply(action, Actor::System, T).is_ok());
    }

    #[test]
    fn checks_of_github_pull_requests_are_not_simulated() {
        let mut r = review(5, vec![]);
        r.check_runs = vec![crate::CheckRun::new("Tests", Checks::Failed, 10, "")];
        let mut ws = synced(&github(vec![pr_item(5)], vec![r]));
        assert!(ws.restart_check(5, "Tests"));
        assert_eq!(ws.running_checks(), 0, "GitHub führt den Check aus");
        assert!(ws.advance_checks().is_empty());
        assert_eq!(ws.review(5).unwrap().check_runs[0].status, Checks::Running);
    }

    #[test]
    fn an_old_state_without_sync_still_loads() {
        let text = persist::save(&demo::workspace());
        let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["workspace"].as_object_mut().unwrap().remove("sync");
        let (ws, restored) = persist::restore(Some(&json.to_string()));
        assert!(restored);
        assert_eq!(ws.sync, SyncState::default());
    }

    #[test]
    fn the_saved_form_has_no_secrets() {
        let ws = synced(&jira(vec![ticket("ABC-1", "Eins")]));
        let text = persist::save(&ws).to_lowercase();
        for word in ["token", "secret", "password", "authorization"] {
            assert!(!text.contains(word), "{word}");
        }
    }
}
