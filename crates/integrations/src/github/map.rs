//! GitHub-Antworten → Typen aus `cockpit-core`. Reine Funktionen, ohne Netzwerk.

use cockpit_core::{
    Checks, Comment, FileDiff, Item, Kind, Person, PrRef, ReasonKind, Review, Scope, Source,
    TicketStatus, Verdict,
};

use super::Relation;
use super::dto::{CheckRun, IssueComment, Pull, PullFile, ReviewComment};
use super::patch::parse_patch;
use crate::{text, zeit};

/// Verdichtet die Check-Läufe eines Commits zu einem Status.
///
/// * ein Lauf fehlgeschlagen (`failure`, `timed_out`, `cancelled`, `action_required`,
///   `startup_failure`) → `Failed`, auch wenn andere noch laufen
/// * sonst ein Lauf nicht abgeschlossen → `Running`
/// * sonst (auch ohne jeden Lauf) → `Passed`
pub fn aggregate_checks(runs: &[CheckRun]) -> Checks {
    let failed = |r: &CheckRun| {
        matches!(
            r.conclusion.as_deref(),
            Some("failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure")
        )
    };
    if runs.iter().any(failed) {
        Checks::Failed
    } else if runs.iter().any(|r| r.status != "completed") {
        Checks::Running
    } else {
        Checks::Passed
    }
}

/// Ein Pull Request als Eintrag der Inbox.
///
/// `repo` ist `owner/name` und geht in die ID ein, damit gleiche Nummern aus verschiedenen
/// Repos sich nicht überschneiden. Die Gründe sind vorläufig: Bei „Review angefragt“ gilt
/// `ReviewRequested`; bei eigenen Pull Requests `ChecksFailed`, sonst `Comment`.
/// Kommentare sind leer – sie kommen über [`comments_to_core`].
pub fn pull_to_item(
    repo: &str,
    pull: &Pull,
    checks: Checks,
    relation: Relation,
    jetzt: i64,
) -> Item {
    let (reason, reason_text) = match (relation, checks) {
        (Relation::ReviewAngefragt, _) => (ReasonKind::ReviewRequested, "Review angefragt"),
        (Relation::Eigener, Checks::Failed) => (ReasonKind::ChecksFailed, "Checks fehlgeschlagen"),
        (Relation::Eigener, _) => (ReasonKind::Comment, "Eigener Pull Request"),
    };
    Item {
        id: text::id_aus(&["gh", repo, &pull.number.to_string()]),
        kind: Kind::PullRequest,
        source: Source::GitHub,
        key: format!("#{}", pull.number),
        title: pull.title.clone(),
        description: pull.body.clone().unwrap_or_default(),
        reason,
        reason_text: reason_text.into(),
        time: zeit::relativ(&pull.updated_at, jetzt),
        scope: Scope::Mine,
        owner: text::person(&pull.user.login),
        status: TicketStatus::Review,
        branch: Some(pull.head.name.clone()),
        pr: Some(PrRef {
            number: pull.number,
            draft: pull.draft,
            checks,
            changes_requested: false,
            merged: false,
        }),
        agent_note: None,
        comments: Vec::new(),
        read: false,
        due: None,
    }
}

/// Patch einer Datei → `FileDiff`. Ohne Patch (Binärdatei, zu groß) bleiben die Zeilen leer.
pub fn file_to_diff(file: &PullFile) -> FileDiff {
    FileDiff {
        name: file.filename.clone(),
        lines: file.patch.as_deref().map(parse_patch).unwrap_or_default(),
    }
}

/// Ein Pull Request mit Dateien als Review. Das Urteil beginnt als „Kommentieren“, es gibt
/// noch keine Vorschläge. Das Ticket kommt aus Branch-Name oder Titel (`BAU-142`).
pub fn pull_to_review(pull: &Pull, files: &[PullFile], checks: Checks) -> Review {
    let ticket_key =
        text::ticket_schluessel(&pull.head.name).or_else(|| text::ticket_schluessel(&pull.title));
    Review {
        number: pull.number,
        title: pull.title.clone(),
        description: pull.body.clone().unwrap_or_default(),
        author: text::person(&pull.user.login),
        branch: pull.head.name.clone(),
        base: pull.base.name.clone(),
        ticket_key,
        checks,
        files: files.iter().map(file_to_diff).collect(),
        agent_reviewed: false,
        suggestions: Vec::new(),
        line_comments: Vec::new(),
        verdict: Verdict::default(),
        sent: false,
        check_runs: Vec::new(),
        merged: None,
    }
}

/// Kommentare des Gesprächs und an Diff-Zeilen, nach Zeit sortiert. Kommentare an Zeilen
/// beginnen mit `datei · Zeile n`. `ich` ist der GitHub-Login des Nutzers.
pub fn comments_to_core(
    issue: &[IssueComment],
    review: &[ReviewComment],
    ich: &str,
    jetzt: i64,
) -> Vec<Comment> {
    let mut alle: Vec<(i64, Comment)> = Vec::new();
    let mut add = |login: &str, zeit_iso: &str, body: String| {
        let eigener = login.eq_ignore_ascii_case(ich);
        let author = if eigener {
            Person::me()
        } else {
            text::person(login)
        };
        let sortierung = zeit::parse_iso(zeit_iso).unwrap_or(0);
        alle.push((
            sortierung,
            Comment {
                author,
                time: zeit::relativ(zeit_iso, jetzt),
                text: body,
                own: eigener,
            },
        ));
    };
    for c in issue {
        add(
            &c.user.login,
            &c.created_at,
            c.body.clone().unwrap_or_default(),
        );
    }
    for c in review {
        let ort = match c.line {
            Some(n) => format!("{} · Zeile {n}", c.path),
            None => c.path.clone(),
        };
        add(
            &c.user.login,
            &c.created_at,
            format!("{ort}\n{}", c.body.as_deref().unwrap_or_default()),
        );
    }
    alle.sort_by_key(|(t, _)| *t);
    alle.into_iter().map(|(_, c)| c).collect()
}
