//! Team-Sicht: wer woran arbeitet und wo etwas hängt.
//!
//! Alles hier sind reine Funktionen über den [`Workspace`]. Sie ändern nichts und
//! kennen keine Oberfläche; die Seite „Übersicht“ zeigt nur ihr Ergebnis.

use crate::model::{Item, Person, TicketStatus, Tone};
use crate::workspace::Workspace;
use crate::{Checks, Kind};

/// So viele offene Einträge gelten als volle Auslastung.
pub const CAPACITY: usize = 4;

/// Wie voll eine Person ist. Immer auch als Text lesbar, nie nur als Farbe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoadLevel {
    /// Nichts oder wenig offen.
    Free,
    /// Gut ausgelastet.
    Busy,
    /// An der Grenze oder darüber.
    Full,
}

impl LoadLevel {
    pub fn label(self) -> &'static str {
        match self {
            LoadLevel::Free => "Hat Luft",
            LoadLevel::Busy => "Gut ausgelastet",
            LoadLevel::Full => "Voll ausgelastet",
        }
    }
}

/// Eine Person mit ihren offenen Einträgen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub person: Person,
    /// Tickets im Status „Offen“.
    pub open: usize,
    /// Tickets im Status „In Arbeit“.
    pub in_progress: usize,
    /// Tickets im Status „Review“.
    pub review: usize,
    /// Offene Pull Requests.
    pub pull_requests: usize,
    /// IDs der Einträge, in der Reihenfolge der Inbox.
    pub item_ids: Vec<String>,
}

impl Member {
    fn new(person: Person) -> Self {
        Self {
            person,
            open: 0,
            in_progress: 0,
            review: 0,
            pull_requests: 0,
            item_ids: Vec::new(),
        }
    }

    pub fn total(&self) -> usize {
        self.open + self.in_progress + self.review + self.pull_requests
    }

    pub fn load_level(&self) -> LoadLevel {
        match self.total() {
            0..=1 => LoadLevel::Free,
            n if n < CAPACITY => LoadLevel::Busy,
            _ => LoadLevel::Full,
        }
    }

    /// Anteil an der Kapazität in Prozent, bei 100 gedeckelt.
    pub fn load_percent(&self) -> usize {
        (self.total() * 100 / CAPACITY).min(100)
    }
}

/// Ein Eintrag zählt, solange er nicht erledigt ist.
fn is_open(item: &Item) -> bool {
    !item.read && item.status != TicketStatus::Done
}

/// Ein Review wartet, wenn es existiert und noch nicht gesendet ist.
fn review_pending(ws: &Workspace, number: u32) -> bool {
    ws.review(number).is_some_and(|r| !r.sent)
}

/// Alle Personen mit ihren offenen Einträgen: erst „Du“ und der Agent, dann die
/// anderen in der Reihenfolge, in der sie in der Inbox auftauchen.
///
/// Der Agent zählt, was er gerade umsetzt (In Arbeit) und was von ihm fertig ist und
/// auf ein Review wartet (Review). Diese Tickets gehören weiter auch ihrem Besitzer.
pub fn members(ws: &Workspace) -> Vec<Member> {
    let mut list = vec![Member::new(Person::me()), Member::new(Person::agent())];
    let slot = |list: &mut Vec<Member>, person: &Person| match list
        .iter()
        .position(|m| m.person == *person)
    {
        Some(i) => i,
        None => {
            list.push(Member::new(person.clone()));
            list.len() - 1
        }
    };

    for item in ws.items.iter().filter(|i| is_open(i)) {
        let i = slot(&mut list, &item.owner);
        let member = &mut list[i];
        match (item.kind, item.status) {
            (Kind::PullRequest, _) => member.pull_requests += 1,
            (Kind::Ticket, TicketStatus::InProgress) => member.in_progress += 1,
            (Kind::Ticket, TicketStatus::Review) => member.review += 1,
            (Kind::Ticket, _) => member.open += 1,
        }
        member.item_ids.push(item.id.clone());
    }

    let agent = &mut list[1];
    // Pro Eintrag zählt der Agent einmal: ein aktiver Lauf (auch ein Folgeauftrag) geht vor
    // einem fertigen, der auf Review wartet.
    let mut active_ids: Vec<String> = Vec::new();
    for run in &ws.runs {
        let Some(item) = ws.item_by_key(&run.ticket_key).filter(|i| is_open(i)) else {
            continue;
        };
        let seen = agent.item_ids.contains(&item.id);
        if run.is_active() {
            if active_ids.contains(&item.id) {
                continue;
            }
            if seen {
                // Der Eintrag war als „Review“ gezählt und ist jetzt wieder in Arbeit.
                agent.review = agent.review.saturating_sub(1);
            } else {
                agent.item_ids.push(item.id.clone());
            }
            active_ids.push(item.id.clone());
            agent.in_progress += 1;
        } else if !seen && run.is_done() && run.pr_number.is_some_and(|n| review_pending(ws, n)) {
            agent.review += 1;
            agent.item_ids.push(item.id.clone());
        }
    }
    list
}

/// Kurzer Stand eines Eintrags für Listen: bei Tickets der Status, bei Pull Requests
/// der Stand von Checks und Review.
pub fn state_label(item: &Item) -> (String, Tone) {
    match (item.kind, &item.pr) {
        (Kind::PullRequest, Some(pr)) => pr.status(),
        (Kind::PullRequest, None) => ("Pull Request".into(), Tone::Gray),
        (Kind::Ticket, _) => {
            let label = TicketStatus::OPTIONS
                .iter()
                .find(|(id, _)| *id == item.status.id())
                .map_or("", |(_, label)| *label);
            let tone = match item.status {
                TicketStatus::Open => Tone::Gray,
                TicketStatus::InProgress => Tone::Blue,
                TicketStatus::Review => Tone::Orange,
                TicketStatus::Done => Tone::Green,
            };
            (label.into(), tone)
        }
    }
}

/// Warum ein Eintrag hängt. Die Reihenfolge ist zugleich die Dringlichkeit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StuckKind {
    ChecksFailed,
    /// Eine Grenze (Budget, Zeitlimit) hat den Agenten angehalten.
    AgentStopped,
    /// Der Agent hat eine Rückfrage und wartet auf die Antwort.
    QuestionWaiting,
    /// Der Agent hat einen roten Check untersucht und wartet, ob er den Fix übernehmen soll.
    DiagnosisWaiting,
    PlanWaiting,
    AgentWaiting,
    ReviewWaiting,
    NoBranch,
}

impl StuckKind {
    pub fn label(self) -> &'static str {
        match self {
            StuckKind::ChecksFailed => "Checks fehlgeschlagen",
            StuckKind::AgentStopped => "Agent angehalten",
            StuckKind::QuestionWaiting => "Frage wartet",
            StuckKind::DiagnosisWaiting => "Diagnose wartet",
            StuckKind::PlanWaiting => "Plan wartet",
            StuckKind::AgentWaiting => "Agent wartet",
            StuckKind::ReviewWaiting => "Review wartet",
            StuckKind::NoBranch => "Ohne Branch",
        }
    }
}

/// Ein Eintrag, der jemanden blockiert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stuck {
    pub item_id: String,
    pub key: String,
    pub title: String,
    pub kind: StuckKind,
    /// Wer etwas tun muss, damit es weitergeht.
    pub who: Person,
    /// Der Grund als ganzer Satz.
    pub reason: String,
    /// Nummer des Pull Requests, falls es zum Review geht.
    pub review: Option<u32>,
    /// Ticket, dessen Agenten-Plan auf Freigabe wartet.
    pub plan: Option<String>,
    /// Ticket, bei dem der Agent eine Frage gestellt hat.
    pub question: Option<String>,
    /// Lauf, dessen Diagnose auf deine Entscheidung wartet.
    pub diagnosis: Option<String>,
}

/// Die erste Regel, die auf den Eintrag passt. Ein Eintrag hängt nur einmal.
fn classify(ws: &Workspace, item: &Item) -> Option<(StuckKind, Person, String)> {
    let me = Person::me();
    let pr = item.pr.as_ref();

    // Die Diagnose geht vor: Der Check ist rot, der Agent hat aber schon einen Vorschlag.
    if let Some(run) = ws
        .runs
        .iter()
        .find(|r| r.ticket_key == item.key && r.is_diagnosed())
    {
        let cause = run
            .diagnosis
            .as_ref()
            .map_or("Der Agent hat den Check untersucht.", |d| d.cause.as_str());
        return Some((
            StuckKind::DiagnosisWaiting,
            me,
            format!("Diagnose des Agenten wartet auf deine Entscheidung: {cause}"),
        ));
    }

    if let Some(pr) = pr.filter(|p| p.checks == Checks::Failed) {
        return Some((
            StuckKind::ChecksFailed,
            item.owner.clone(),
            format!(
                "Die Checks von #{} sind fehlgeschlagen – {} kommt nicht weiter.",
                pr.number,
                who_name(&item.owner)
            ),
        ));
    }

    if let Some(reason) = ws
        .runs
        .iter()
        .find(|r| r.ticket_key == item.key)
        .and_then(|r| r.stop_reason())
    {
        return Some((
            StuckKind::AgentStopped,
            me,
            format!(
                "Der Agent ist angehalten: {}. Erhöhe das Limit und setze ihn fort – oder brich ab.",
                reason.label()
            ),
        ));
    }

    if let Some(question) = ws
        .runs
        .iter()
        .find(|r| r.ticket_key == item.key && r.is_asking())
        .and_then(|r| r.question.as_ref())
    {
        return Some((
            StuckKind::QuestionWaiting,
            me,
            format!("Der Agent fragt: „{}“", question.text),
        ));
    }

    if ws
        .runs
        .iter()
        .any(|r| r.ticket_key == item.key && r.is_awaiting())
    {
        return Some((
            StuckKind::PlanWaiting,
            me,
            "Der Agent hat einen Plan vorgelegt und wartet auf deine Freigabe.".to_string(),
        ));
    }

    let waiting_run = ws.runs.iter().find(|r| {
        r.ticket_key == item.key
            && r.is_done()
            && r.pr_number.is_some_and(|n| review_pending(ws, n))
    });
    if let Some(number) = waiting_run.and_then(|r| r.pr_number) {
        return Some((
            StuckKind::AgentWaiting,
            me,
            format!("Der Agent ist fertig und wartet auf dein Review von #{number}."),
        ));
    }

    if let Some(pr) = pr.filter(|p| {
        item.kind == Kind::PullRequest
            && item.owner != me
            && !p.draft
            && p.checks == Checks::Passed
            && review_pending(ws, p.number)
    }) {
        return Some((
            StuckKind::ReviewWaiting,
            me,
            format!(
                "{} wartet auf dein Review von #{}.",
                item.owner.name, pr.number
            ),
        ));
    }

    // Arbeitet der Agent gerade daran, legt er den Branch selbst an.
    let agent_working = ws
        .runs
        .iter()
        .any(|r| r.ticket_key == item.key && r.is_active());
    if item.is_ticket()
        && item.status == TicketStatus::InProgress
        && item.branch.is_none()
        && !agent_working
    {
        return Some((
            StuckKind::NoBranch,
            item.owner.clone(),
            format!(
                "Das Ticket ist in Arbeit, hat aber noch keinen Branch – {}.",
                if item.owner == me {
                    "lege einen an".to_string()
                } else {
                    format!("{} muss einen anlegen", item.owner.name)
                }
            ),
        ));
    }
    None
}

/// „Du“ im Satz klingt falsch („Du kommt nicht weiter“), also wird angepasst.
fn who_name(person: &Person) -> String {
    if *person == Person::me() {
        "du".into()
    } else {
        person.name.clone()
    }
}

/// Einträge, die hängen, die dringendsten zuerst. Erledigte Einträge zählen nicht.
pub fn stuck(ws: &Workspace) -> Vec<Stuck> {
    let mut list: Vec<Stuck> = ws
        .items
        .iter()
        .filter(|i| is_open(i))
        .filter_map(|item| {
            let (kind, who, reason) = classify(ws, item)?;
            // Der Schlüssel des Laufs, der wartet (bei Folgeaufträgen nicht der des Eintrags).
            let waiting = |ok: fn(&crate::AgentRun) -> bool| {
                ws.runs
                    .iter()
                    .find(|r| r.ticket_key == item.key && ok(r))
                    .map(crate::AgentRun::key)
            };
            Some(Stuck {
                item_id: item.id.clone(),
                key: item.key.clone(),
                title: item.title.clone(),
                kind,
                who,
                reason,
                review: item.pr.as_ref().map(|p| p.number),
                plan: waiting(crate::AgentRun::is_awaiting)
                    .filter(|_| kind == StuckKind::PlanWaiting),
                question: waiting(crate::AgentRun::is_asking)
                    .filter(|_| kind == StuckKind::QuestionWaiting),
                diagnosis: waiting(crate::AgentRun::is_diagnosed)
                    .filter(|_| kind == StuckKind::DiagnosisWaiting),
            })
        })
        .collect();
    // Stabil: gleich dringende bleiben in der Reihenfolge der Inbox.
    list.sort_by_key(|s| s.kind);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentOptions, demo};

    /// Ohne Plan-Schritt: Der Agent legt sofort los.
    fn direct() -> AgentOptions {
        AgentOptions {
            plan_first: false,
            ..AgentOptions::default()
        }
    }

    #[test]
    fn a_plan_waiting_for_approval_is_stuck_with_me() {
        let mut w = demo::workspace();
        w.set_status("bau150", TicketStatus::InProgress);
        w.start_agent("BAU-150", AgentOptions::default()).unwrap();
        let s = stuck(&w);
        let entry = s.iter().find(|s| s.key == "BAU-150").unwrap();
        assert_eq!(entry.kind, StuckKind::PlanWaiting);
        assert_eq!(entry.who, Person::me());
        assert_eq!(entry.plan.as_deref(), Some("BAU-150"));
        // Der wartende Agent belegt das Ticket.
        assert_eq!(member(&members(&w), "Agent").in_progress, 1);
        // Nach der Freigabe läuft er – dann hängt nichts mehr.
        assert!(w.approve_plan("BAU-150"));
        assert!(stuck(&w).iter().all(|s| s.key != "BAU-150"));
    }

    fn member<'a>(list: &'a [Member], name: &str) -> &'a Member {
        list.iter().find(|m| m.person.name == name).unwrap()
    }

    #[test]
    fn demo_members_are_grouped_by_person() {
        let list = members(&demo::workspace());
        let names: Vec<_> = list.iter().map(|m| m.person.name.as_str()).collect();
        assert_eq!(names, ["Du", "Agent", "Jonas", "Mira"]);

        let me = member(&list, "Du");
        assert_eq!(
            (me.open, me.in_progress, me.review, me.pull_requests),
            (1, 1, 1, 0)
        );
        assert_eq!(me.item_ids, ["bau142", "bau138", "bau150"]);
        // Der Agent hat BAU-138 fertig, das Review wartet.
        let agent = member(&list, "Agent");
        assert_eq!((agent.in_progress, agent.review), (0, 1));
        assert_eq!(agent.item_ids, ["bau138"]);
        assert_eq!(member(&list, "Jonas").pull_requests, 1);
        assert_eq!(member(&list, "Mira").item_ids, ["pr85"]);
    }

    #[test]
    fn me_and_agent_are_always_listed() {
        let list = members(&Workspace::default());
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|m| m.total() == 0));
        assert_eq!(list[0].load_level(), LoadLevel::Free);
        assert_eq!(list[0].load_percent(), 0);
    }

    #[test]
    fn load_grows_with_open_entries() {
        let mut m = Member::new(Person::me());
        let mut levels = Vec::new();
        for _ in 0..=CAPACITY + 1 {
            levels.push((m.load_level(), m.load_percent()));
            m.open += 1;
        }
        assert_eq!(levels[0], (LoadLevel::Free, 0));
        assert_eq!(levels[1], (LoadLevel::Free, 25));
        assert_eq!(levels[2], (LoadLevel::Busy, 50));
        assert_eq!(levels[3], (LoadLevel::Busy, 75));
        assert_eq!(levels[CAPACITY], (LoadLevel::Full, 100));
        // Über die Kapazität hinaus bleibt der Balken bei 100.
        assert_eq!(levels[CAPACITY + 1], (LoadLevel::Full, 100));
    }

    #[test]
    fn done_entries_do_not_count() {
        let mut w = demo::workspace();
        w.mark_done("bau150");
        w.set_status("bau142", TicketStatus::Done);
        let list = members(&w);
        assert_eq!(member(&list, "Du").item_ids, ["bau138"]);
    }

    #[test]
    fn running_agent_counts_as_in_progress() {
        let mut w = demo::workspace();
        w.start_agent("BAU-150", direct()).unwrap();
        let list = members(&w);
        let agent = member(&list, "Agent");
        assert_eq!((agent.in_progress, agent.review), (1, 1));
        assert_eq!(agent.item_ids, ["bau138", "bau150"]);
        // Abgebrochen zählt er nicht mehr.
        w.cancel_agent("BAU-150");
        assert_eq!(member(&members(&w), "Agent").item_ids, ["bau138"]);
    }

    #[test]
    fn demo_stuck_entries_in_order_of_urgency() {
        let list = stuck(&demo::workspace());
        let found: Vec<_> = list.iter().map(|s| (s.key.as_str(), s.kind)).collect();
        assert_eq!(
            found,
            [
                ("#85", StuckKind::ChecksFailed),
                ("BAU-138", StuckKind::AgentWaiting),
                ("#87", StuckKind::ReviewWaiting),
                ("BAU-142", StuckKind::NoBranch),
            ]
        );
        // BAU-150 ist nur offen und hängt nicht.
        assert!(list.iter().all(|s| s.key != "BAU-150"));
    }

    #[test]
    fn stuck_entries_name_reason_and_target() {
        let list = stuck(&demo::workspace());
        let by_key = |k: &str| list.iter().find(|s| s.key == k).unwrap();

        let failed = by_key("#85");
        assert_eq!(failed.who.name, "Mira");
        assert!(failed.reason.contains("Mira"));
        assert_eq!(failed.review, Some(85));

        let agent = by_key("BAU-138");
        assert_eq!(agent.who, Person::me());
        assert!(agent.reason.contains("#91"));
        assert_eq!(agent.review, Some(91));

        let review = by_key("#87");
        assert!(review.reason.starts_with("Jonas wartet"));

        let branch = by_key("BAU-142");
        assert_eq!(branch.review, None);
        assert!(branch.reason.contains("keinen Branch"));
    }

    #[test]
    fn creating_a_branch_unblocks_the_ticket() {
        let mut w = demo::workspace();
        w.create_branch("bau142");
        assert!(stuck(&w).iter().all(|s| s.key != "BAU-142"));
    }

    #[test]
    fn sending_the_review_unblocks_the_pull_request() {
        let mut w = demo::workspace();
        for s in ["a", "b", "c"] {
            w.decide(87, s, Some(crate::Decision::Approved));
        }
        w.send_review(87, "").unwrap();
        assert!(stuck(&w).iter().all(|s| s.key != "#87"));
        // Bei Jonas ist dann nichts mehr offen; wer nichts hat, steht nicht mehr da.
        assert!(
            members(&w)
                .iter()
                .all(|m| m.person.name != "Jonas" || m.total() == 0)
        );
    }

    #[test]
    fn finished_agent_run_waits_for_review_until_sent() {
        let mut w = demo::workspace();
        w.start_agent("BAU-150", direct()).unwrap();
        // Läuft er noch, hängt nichts – auch wenn der Branch erst am Ende entsteht.
        assert!(stuck(&w).iter().all(|s| s.key != "BAU-150"));
        for _ in 0..crate::agent::STEPS {
            w.advance_agents();
        }
        let s = stuck(&w);
        let entry = s.iter().find(|s| s.key == "BAU-150").unwrap();
        assert_eq!(entry.kind, StuckKind::AgentWaiting);
        assert_eq!(entry.who, Person::me());
    }

    #[test]
    fn failed_checks_win_over_other_reasons() {
        let mut w = demo::workspace();
        // PR 87 wartet auf Review – schlagen die Checks fehl, ist das der Grund.
        w.items
            .iter_mut()
            .find(|i| i.id == "pr87")
            .and_then(|i| i.pr.as_mut())
            .unwrap()
            .checks = Checks::Failed;
        let list = stuck(&w);
        let entry = list.iter().find(|s| s.key == "#87").unwrap();
        assert_eq!(entry.kind, StuckKind::ChecksFailed);
        assert_eq!(list.iter().filter(|s| s.key == "#87").count(), 1);
    }

    #[test]
    fn drafts_and_running_checks_do_not_hang() {
        let mut w = demo::workspace();
        let pr = w
            .items
            .iter_mut()
            .find(|i| i.id == "pr87")
            .and_then(|i| i.pr.as_mut())
            .unwrap();
        pr.draft = true;
        assert!(stuck(&w).iter().all(|s| s.key != "#87"));
        let pr = w
            .items
            .iter_mut()
            .find(|i| i.id == "pr87")
            .and_then(|i| i.pr.as_mut())
            .unwrap();
        pr.draft = false;
        pr.checks = Checks::Running;
        assert!(stuck(&w).iter().all(|s| s.key != "#87"));
    }

    #[test]
    fn state_label_for_tickets_and_pull_requests() {
        let w = demo::workspace();
        let label = |id: &str| state_label(w.item(id).unwrap());
        assert_eq!(label("bau150"), ("Offen".to_string(), Tone::Gray));
        assert_eq!(label("bau142"), ("In Arbeit".to_string(), Tone::Blue));
        assert_eq!(label("bau138"), ("Review".to_string(), Tone::Orange));
        assert_eq!(label("pr85").1, Tone::Red);
        assert_eq!(label("pr87").1, Tone::Orange);
    }

    #[test]
    fn nothing_hangs_in_an_empty_workspace() {
        assert!(stuck(&Workspace::default()).is_empty());
    }

    #[test]
    fn own_entries_read_naturally() {
        let mut w = demo::workspace();
        w.items.iter_mut().find(|i| i.id == "pr85").unwrap().owner = Person::me();
        let list = stuck(&w);
        let entry = list.iter().find(|s| s.key == "#85").unwrap();
        assert!(entry.reason.contains("du kommt nicht weiter"));
    }

    #[test]
    fn a_waiting_question_is_stuck_with_me() {
        let mut w = demo::workspace();
        w.start_agent("BAU-142", direct()).unwrap();
        for _ in 0..3 {
            w.advance_agents();
        }
        let s = stuck(&w);
        let entry = s.iter().find(|s| s.key == "BAU-142").unwrap();
        assert_eq!(entry.kind, StuckKind::QuestionWaiting);
        assert_eq!(entry.kind.label(), "Frage wartet");
        assert_eq!(entry.who, Person::me());
        assert_eq!(entry.question.as_deref(), Some("BAU-142"));
        assert!(entry.plan.is_none());
        assert!(entry.reason.contains("deprecated"), "{}", entry.reason);
        assert_eq!(w.waiting_agents(), 1, "zählt in der Sidebar bei Agenten");
        // Eine Frage ist dringender als ein wartender Plan.
        assert!(StuckKind::QuestionWaiting < StuckKind::PlanWaiting);
        // Nach der Antwort hängt nichts mehr.
        assert!(w.answer_agent("BAU-142", "Entfernen"));
        assert!(stuck(&w).iter().all(|s| s.key != "BAU-142"));
        assert_eq!(w.waiting_agents(), 0);
    }
}
