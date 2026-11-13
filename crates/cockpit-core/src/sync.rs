//! Sync with Jira and GitHub: what the services report flows into the workspace.
//!
//! Everything here is pure – no network, no clock. The backend fetches the data
//! (`cockpit-integrations`), packs it into a [`Remote`] and applies it as the action
//! [`Action::SyncRemote`](crate::Action::SyncRemote). [`merge`] combines both:
//!
//! * **New** entries are added (at the front), **removed** ones disappear.
//! * **Local state stays**: read/done, snoozes, agent runs and
//!   hints of the agent, suggestions and decisions in the review, own not yet
//!   sent line comments, verdict and „sent“.
//! * **Changed from outside** (new comments, status, checks, requested changes)
//!   appear as [`Change`] – the log gets entries for them with author `System`,
//!   and `notify::events` sees them in the before/after like any other change.
//! * If a source fails (token expired, network gone, quota), its data stays
//!   unchanged; the state per source ([`SourceState`]) records what the cause is.
//!
//! On the first successful sync the sample data disappear (only the
//! unchanged parts of the demo), so that real data is not mixed with them.

use serde::{Deserialize, Serialize};

use crate::action::Target;
use crate::agent::AgentRun;
use crate::demo;
use crate::model::{Checks, Item, PrKey, Source, TicketStatus};
use crate::review::{FileDiff, LineComment, Review};
use crate::workspace::Workspace;

/// This many new or removed entries appear individually in the log; above that a collective line.
const MAX_SINGLE: usize = 5;
/// The state keeps this many hints per source.
const MAX_WARNINGS: usize = 5;

// ---------- What the services deliver ----------

/// The state of both services after a fetch. A source that is missing was not queried
/// (not configured, no key, pause because of quota) – its data stays.
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
    /// The complete state of the source. What is missing here has disappeared there.
    Loaded(Loaded),
    /// The fetch failed.
    Failed(Problem),
}

/// The complete state of a source.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    #[serde(default)]
    pub items: Vec<Item>,
    /// GitHub only: the pull requests as a review (diff, checks).
    #[serde(default)]
    pub reviews: Vec<Review>,
    /// GitHub only: which number belongs to which repo.
    #[serde(default)]
    pub prs: Vec<PrLink>,
    /// Hints for the state of the source (e.g. skipped pull requests).
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Which repo a pull request belongs to (and its last known commit). Reviews carry the
/// repo themselves; the link is what the backend needs for write access.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrLink {
    pub number: u32,
    /// `owner/name`
    pub repo: String,
    /// Last known commit of the branch (saves the diff on the next fetch).
    #[serde(default)]
    pub head_sha: String,
}

impl PrLink {
    pub fn key(&self) -> PrKey {
        PrKey::new(&self.repo, self.number)
    }
}

// ---------- State per source ----------

/// What a fetch failed on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// 401: token missing, expired or revoked.
    Unauthorized,
    /// 403: token valid, but without permission.
    NoPermission,
    /// Quota used up; until then (milliseconds since 1970) nothing is queried.
    RateLimited { until_ms: Option<u64> },
    /// No connection to the service.
    Offline,
    /// Everything else, with a German message (without key).
    Other(String),
}

impl Problem {
    pub fn text(&self) -> String {
        match self {
            Problem::Unauthorized => {
                "Token abgelehnt – abgelaufen oder widerrufen. Unter „Einrichten“ erneuern".into()
            }
            Problem::NoPermission => {
                "Dem Token fehlt eine Berechtigung – unter „Einrichten“ prüfen, was er darf".into()
            }
            Problem::RateLimited { until_ms: Some(_) } => {
                "Kontingent aufgebraucht – der Abgleich pausiert".into()
            }
            Problem::RateLimited { until_ms: None } => "Kontingent aufgebraucht".into(),
            Problem::Offline => "Keine Verbindung zum Dienst".into(),
            Problem::Other(m) => m.clone(),
        }
    }
}

/// How the sync of a source stands. If the source is missing from the list, it is off.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SourceState {
    pub source: Source,
    /// `None` = the last fetch was successful.
    #[serde(default)]
    pub problem: Option<Problem>,
    /// Last successful sync (milliseconds since 1970).
    #[serde(default)]
    pub last_ok_ms: Option<u64>,
    /// Last attempt.
    #[serde(default)]
    pub last_try_ms: u64,
    /// Since when the current problem lasts (milliseconds since 1970); `None` while all is well.
    /// Stays while the same kind of problem repeats, so "seit 10 Min." is stable.
    #[serde(default)]
    pub since_ms: Option<u64>,
    /// Hints from the last successful fetch.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl SourceState {
    pub fn is_ok(&self) -> bool {
        self.problem.is_none()
    }

    /// Short state statement for the UI and `/api/health`.
    pub fn id(&self) -> &'static str {
        match &self.problem {
            None => "ok",
            Some(Problem::Unauthorized) => "unauthorized",
            Some(Problem::NoPermission) => "no_permission",
            Some(Problem::RateLimited { .. }) => "quota",
            Some(Problem::Offline) => "offline",
            Some(Problem::Other(_)) => "error",
        }
    }
}

/// Everything the sync records in the workspace.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncState {
    /// State per queried source.
    #[serde(default)]
    pub sources: Vec<SourceState>,
    /// IDs of the entries that come from a service. Only for them do actions go to the outside.
    #[serde(default)]
    pub items: Vec<String>,
    /// Pull requests that come from GitHub.
    #[serde(default)]
    pub prs: Vec<PrLink>,
    /// The sample data are already removed.
    #[serde(default)]
    pub demo_dropped: bool,
}

impl SyncState {
    pub fn state_of(&self, source: Source) -> Option<&SourceState> {
        self.sources.iter().find(|s| s.source == source)
    }

    /// Does this entry come from a service (and not from the sample data)?
    pub fn is_synced(&self, item_id: &str) -> bool {
        self.items.iter().any(|i| i == item_id)
    }

    /// The link of a pull request; a plain number finds the first with that number.
    pub fn pr(&self, key: impl Into<PrKey>) -> Option<&PrLink> {
        let key = key.into();
        self.prs.iter().find(|p| key.matches(&p.repo, p.number))
    }

    /// Until when does the sync of this source pause because of the quota?
    pub fn paused_until(&self, source: Source, now_ms: u64) -> Option<u64> {
        match self.state_of(source)?.problem {
            Some(Problem::RateLimited { until_ms: Some(t) }) if t > now_ms => Some(t),
            _ => None,
        }
    }

    pub(crate) fn set_state(
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
                    since_ms: None,
                    warnings: Vec::new(),
                });
                self.sources.last_mut().expect("just inserted")
            }
        };
        entry.last_try_ms = now_ms;
        if problem.is_none() {
            entry.last_ok_ms = Some(now_ms);
            entry.warnings = warn.iter().take(MAX_WARNINGS).cloned().collect();
        }
        entry.since_ms = match (&problem, &entry.problem) {
            (None, _) => None,
            (Some(new), Some(old))
                if std::mem::discriminant(new) == std::mem::discriminant(old) =>
            {
                entry.since_ms.or(Some(now_ms))
            }
            (Some(_), _) => Some(now_ms),
        };
        entry.problem = problem;
    }
}

// ---------- Changes ----------

/// What changed from outside during the sync. The log entries arise from it.
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
        number: PrKey,
        to: Checks,
    },
    ChangesRequested {
        id: String,
        number: PrKey,
    },
    /// An own, not yet sent comment no longer fits the diff after new commits.
    DraftDropped {
        number: PrKey,
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
    /// Collective line when many entries come or go at once.
    Summary(String),
}

impl Change {
    /// Short German sentence for the log.
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
                format!("Checks für {number}: {}", to.label())
            }
            Change::ChangesRequested { number, .. } => {
                format!("Änderungen angefordert in {number}")
            }
            Change::DraftDropped {
                number,
                place,
                text,
            } => format!(
                "Dein Entwurf an {place} in {number} passt nicht mehr zum geänderten Diff und entfällt: „{text}“"
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
            Change::DraftDropped { number, .. } => Some(Target::Review(number.clone())),
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

// ---------- Merging ----------

/// Merges the state of the services with the local workspace. Pure: `local` stays
/// unchanged, `now_ms` comes from outside.
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
    // Snoozes of entries that no longer exist are cleaned up.
    let items = &ws.items;
    ws.snoozed.retain(|s| items.iter().any(|i| i.id == s.item));
    (ws, changes)
}

/// Removes what still comes exactly like this from the sample data.
fn drop_demo(ws: &mut Workspace) {
    let demo = demo::workspace();
    ws.items
        .retain(|i| !demo.items.iter().any(|d| d.id == i.id) || ws.sync.is_synced(&i.id));
    ws.reviews.retain(|r| {
        ws.sync.pr(r.key()).is_some() || !demo.reviews.iter().any(|d| d.key() == r.key())
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

    // Existing entries of this source: update or remove.
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
    // New entries come to the front, in the order of the service.
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

    // The list of entries that come from services.
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

/// Up to [`MAX_SINGLE`] individually, above that a collective line (the first change as carrier).
fn collapse(list: Vec<Change>, out: &mut Vec<Change>, noun: &str) {
    if list.len() <= MAX_SINGLE {
        out.extend(list);
        return;
    }
    out.push(Change::Summary(format!("{} {noun}", list.len())));
}

/// The local entry `old` gets the values of the service; local state stays.
fn merge_item(old: Item, remote: &Item, changes: &mut Vec<Change>) -> Item {
    let mut item = remote.clone();

    // Local stays: read, hint of the agent, due date without counterpart in the service,
    // branch for tickets (Jira does not know it).
    item.read = old.read;
    item.agent_note = old.agent_note.clone();
    if item.due.is_none() && remote.source == Source::GitHub {
        item.due = old.due.clone();
    }
    if item.branch.is_none() {
        item.branch = old.branch.clone();
    }
    // Priority and link to the pull request are maintained by Cockpit (Jira delivers neither
    // today): what you set stays until the service itself reports a value.
    if item.priority == crate::model::Priority::default() {
        item.priority = old.priority;
    }
    if item.is_ticket() && item.pr.is_none() {
        item.pr = old.pr.clone();
    }
    // The agent's result is a local reason; it stays until the service says something new.
    if old.reason == crate::model::ReasonKind::AgentFinished && old.is_ticket() {
        item.reason = old.reason;
        item.reason_text = old.reason_text.clone();
    }

    let mut bump = false;

    // New comments from others.
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
                number: new.key(),
                to: new.checks,
            });
            bump = true;
        }
        if new.changes_requested && !prev.changes_requested {
            changes.push(Change::ChangesRequested {
                id: item.id.clone(),
                number: new.key(),
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
        if ws.sync.pr(review.key()).is_some() {
            by_number.push(review);
        } else {
            kept.push(review);
        }
    }
    // Reviews that no longer exist on GitHub disappear (only linked ones).
    let mut merged: Vec<Review> = Vec::new();
    for r in &loaded.reviews {
        let previous = by_number.iter().position(|o| o.key().same(&r.key()));
        let review = match previous {
            Some(i) => merge_review(by_number.remove(i), r, changes),
            None => {
                // Number already taken locally (example or simulation): the service wins.
                kept.retain(|k| !k.key().same(&r.key()));
                r.clone()
            }
        };
        merged.push(review);
    }
    // New reviews to the front, existing ones in their place as before: simply before the rest.
    merged.append(&mut kept);
    ws.reviews = merged;

    ws.sync.prs = loaded.prs.clone();
}

/// The local review `old` gets diff, title and checks of the service.
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

/// Transfer own drafts into the new diff. The position is an index into the lines
/// of the file; if the diff changes, we look for the same line again (kind, number, text).
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
        // The same line (kind and text; the number shifts) – with several equal ones
        // the equally-numbered one.
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
                number: old.key(),
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
mod tests;
