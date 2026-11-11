//! Der Workspace: alle Daten des Cockpits und die Aktionen der Oberfläche.

use serde::{Deserialize, Serialize};

use crate::agent::{AgentOptions, AgentRun};
use crate::demo;
use crate::model::*;
use crate::review::{Decision, Review, SendError, Verdict};
use crate::text::slug;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentError {
    UnknownTicket,
    AlreadyRunning,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Workspace {
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub reviews: Vec<Review>,
    #[serde(default)]
    pub runs: Vec<AgentRun>,
}

impl Workspace {
    // ---------- Lesen ----------

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_by_key(&self, key: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.key == key)
    }

    fn item_mut(&mut self, id: &str) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// Einträge der Inbox für einen Filter, in Anzeigereihenfolge.
    pub fn visible(&self, filter: Filter) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| filter.matches(i.scope))
            .collect()
    }

    /// Einträge der Inbox nach Besitzer, Quelle und Suchtext.
    pub fn search(&self, filter: Filter, source: SourceFilter, query: &str) -> Vec<&Item> {
        crate::search::search(&self.items, filter, source, query)
    }

    /// Einträge, die noch Aufmerksamkeit brauchen.
    pub fn unread(&self, filter: Filter) -> usize {
        self.visible(filter).iter().filter(|i| !i.read).count()
    }

    pub fn review(&self, number: u32) -> Option<&Review> {
        self.reviews.iter().find(|r| r.number == number)
    }

    fn review_mut(&mut self, number: u32) -> Option<&mut Review> {
        self.reviews.iter_mut().find(|r| r.number == number)
    }

    /// Reviews, die noch nicht gesendet sind.
    pub fn open_reviews(&self) -> usize {
        self.reviews.iter().filter(|r| !r.sent).count()
    }

    pub fn running_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_running()).count()
    }

    /// Agenten, die einen Plan vorgelegt haben und auf den Nutzer warten.
    pub fn waiting_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_awaiting()).count()
    }

    /// Nummer, die der nächste Pull Request bekommt.
    pub fn next_pr_number(&self) -> u32 {
        let reviews = self.reviews.iter().map(|r| r.number);
        let items = self
            .items
            .iter()
            .filter_map(|i| i.pr.as_ref().map(|p| p.number));
        let runs = self.runs.iter().filter_map(|r| r.pr_number);
        reviews.chain(items).chain(runs).max().unwrap_or(0) + 1
    }

    // ---------- Tickets und Pull Requests ----------

    pub fn mark_done(&mut self, id: &str) {
        if let Some(item) = self.item_mut(id) {
            item.read = true;
        }
    }

    /// Ändert den Status eines Tickets. Pull Requests haben keinen.
    pub fn set_status(&mut self, id: &str, status: TicketStatus) -> bool {
        match self.item_mut(id) {
            Some(item) if item.is_ticket() => {
                item.status = status;
                true
            }
            _ => false,
        }
    }

    /// Legt für einen Eintrag ohne Branch einen an und gibt den Namen zurück.
    pub fn create_branch(&mut self, id: &str) -> Option<String> {
        let item = self.item_mut(id).filter(|i| i.branch.is_none())?;
        let name = format!("feature/{}-{}", item.key, slug(&item.title, 24));
        item.branch = Some(name.clone());
        Some(name)
    }

    /// Antwortet im Ticket oder Pull Request. Leerer Text wird ignoriert.
    pub fn reply(&mut self, id: &str, text: &str) -> bool {
        let text = text.trim();
        let Some(item) = self.item_mut(id).filter(|_| !text.is_empty()) else {
            return false;
        };
        item.comments.push(Comment {
            own: true,
            ..Comment::new(Person::me(), "gerade eben", text)
        });
        true
    }

    // ---------- Review ----------

    pub fn decide(&mut self, number: u32, suggestion: &str, decision: Option<Decision>) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.decide(suggestion, decision))
    }

    pub fn edit_suggestion(&mut self, number: u32, suggestion: &str, text: &str) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.edit(suggestion, text))
    }

    /// Eigener Kommentar an einer Diff-Zeile. Gibt die ID des Kommentars zurück.
    pub fn add_line_comment(
        &mut self,
        number: u32,
        file: &str,
        line: usize,
        text: &str,
    ) -> Option<String> {
        self.review_mut(number)?.add_line_comment(file, line, text)
    }

    pub fn edit_line_comment(&mut self, number: u32, comment: &str, text: &str) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.edit_line_comment(comment, text))
    }

    pub fn delete_line_comment(&mut self, number: u32, comment: &str) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.delete_line_comment(comment))
    }

    pub fn set_verdict(&mut self, number: u32, verdict: Verdict) {
        if let Some(r) = self.review_mut(number) {
            r.set_verdict(verdict);
        }
    }

    /// Lässt den Agenten einen Pull Request durchsehen. Gibt die Zahl der Vorschläge zurück.
    pub fn request_agent_review(&mut self, number: u32) -> usize {
        let count = self.review_mut(number).map_or(0, Review::run_agent_review);
        if let Some(item) = self.pr_item_mut(number) {
            item.agent_note = Some(match count {
                0 => "Der Agent hat nichts zu beanstanden".to_string(),
                1 => "1 Kommentar vom Agenten wartet auf deine Freigabe".to_string(),
                n => format!("{n} Kommentare vom Agenten warten auf deine Freigabe"),
            });
        }
        count
    }

    /// Der Eintrag, der für diesen Pull Request in der Inbox steht.
    fn pr_item_mut(&mut self, number: u32) -> Option<&mut Item> {
        self.items
            .iter_mut()
            .find(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
    }

    /// Sendet das Review. Der Eintrag in der Inbox bekommt eine Zusammenfassung als
    /// eigenen Kommentar und gilt als erledigt. Gibt die Zahl der Kommentare zurück.
    pub fn send_review(&mut self, number: u32, own_comment: &str) -> Result<usize, SendError> {
        let review = self.review_mut(number).ok_or(SendError::UnknownReview)?;
        let verdict = review.verdict;
        let sent = review.send()?.len();

        let mut text = format!(
            "Review gesendet: {} · {sent} {}",
            verdict.label(),
            if sent == 1 { "Kommentar" } else { "Kommentare" }
        );
        if !own_comment.trim().is_empty() {
            text.push('\n');
            text.push_str(own_comment.trim());
        }
        if let Some(item) = self.pr_item_mut(number) {
            item.comments.push(Comment {
                own: true,
                ..Comment::new(Person::me(), "gerade eben", &text)
            });
            item.agent_note = None;
            item.read = true;
        }
        Ok(sent)
    }

    // ---------- Agenten ----------

    /// Der Lauf zu einem Ticket. Gibt es noch keinen, kommt ein startbereiter zurück.
    pub fn run(&self, ticket_key: &str) -> Option<AgentRun> {
        if let Some(run) = self.runs.iter().find(|r| r.ticket_key == ticket_key) {
            return Some(run.clone());
        }
        let item = self.item_by_key(ticket_key).filter(|i| i.is_ticket())?;
        let mut run = AgentRun::new(&item.key, &item.title);
        run.comments = item.comments.len();
        run.files = demo::agent_changes(&item.key)
            .into_iter()
            .map(|f| f.name)
            .collect();
        Some(run)
    }

    /// Beauftragt den Agenten mit einem Ticket.
    pub fn start_agent(
        &mut self,
        ticket_key: &str,
        options: AgentOptions,
    ) -> Result<(), AgentError> {
        let mut run = self.run(ticket_key).ok_or(AgentError::UnknownTicket)?;
        if run.is_active() {
            return Err(AgentError::AlreadyRunning);
        }
        run.start(options, self.next_pr_number());
        let waiting = run.is_awaiting();
        match self.runs.iter_mut().find(|r| r.ticket_key == ticket_key) {
            Some(slot) => *slot = run,
            None => self.runs.push(run),
        }
        if let Some(item) = self.items.iter_mut().find(|i| i.key == ticket_key) {
            // Wartet der Agent noch auf die Freigabe, bleibt das Ticket unverändert.
            if waiting {
                item.agent_note = Some("Der Agent wartet auf die Freigabe seines Plans".into());
            } else {
                item.status = TicketStatus::InProgress;
                item.agent_note = Some("Der Agent arbeitet an der Umsetzung".into());
            }
        }
        Ok(())
    }

    fn active_run_mut(&mut self, ticket_key: &str) -> Option<&mut AgentRun> {
        self.runs
            .iter_mut()
            .find(|r| r.ticket_key == ticket_key && r.is_active())
    }

    fn clear_agent_note(&mut self, ticket_key: &str) {
        if let Some(item) = self.items.iter_mut().find(|i| i.key == ticket_key) {
            item.agent_note = None;
        }
    }

    /// Bricht einen wartenden oder laufenden Agenten ab.
    pub fn cancel_agent(&mut self, ticket_key: &str) {
        let Some(run) = self.active_run_mut(ticket_key) else {
            return;
        };
        run.reset();
        self.clear_agent_note(ticket_key);
    }

    /// Gibt den Plan frei: Der Agent legt los, das Ticket geht auf „In Arbeit“.
    pub fn approve_plan(&mut self, ticket_key: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::approve)
        {
            return false;
        }
        if let Some(item) = self.items.iter_mut().find(|i| i.key == ticket_key) {
            item.status = TicketStatus::InProgress;
            item.agent_note = Some("Der Agent arbeitet an der Umsetzung".into());
        }
        true
    }

    /// Lehnt den Plan ab. Der Lauf endet ohne Spuren.
    pub fn reject_plan(&mut self, ticket_key: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::reject)
        {
            return false;
        }
        self.clear_agent_note(ticket_key);
        true
    }

    /// Fordert mit einer Anmerkung einen neuen Plan an.
    pub fn revise_plan(&mut self, ticket_key: &str, note: &str) -> bool {
        self.active_run_mut(ticket_key)
            .is_some_and(|r| r.revise(note))
    }

    /// Lässt jeden laufenden Agenten einen Schritt abschließen. Gibt die Tickets
    /// zurück, deren Lauf dabei fertig geworden ist.
    pub fn advance_agents(&mut self) -> Vec<String> {
        let finished: Vec<AgentRun> = self
            .runs
            .iter_mut()
            .filter_map(|run| run.advance().then(|| run.clone()))
            .collect();
        for run in &finished {
            self.publish(run);
        }
        finished.into_iter().map(|r| r.ticket_key).collect()
    }

    /// Ein Lauf ist fertig: Branch und Pull Request erscheinen am Ticket, und der
    /// Pull Request wartet als Review.
    fn publish(&mut self, run: &AgentRun) {
        let Some(number) = run.pr_number else { return };
        let checks = if run.options.run_tests {
            Checks::Passed
        } else {
            Checks::Running
        };
        let tests = if run.options.run_tests {
            "Tests bestanden"
        } else {
            "Tests übersprungen"
        };

        if let Some(item) = self.items.iter_mut().find(|i| i.key == run.ticket_key) {
            item.branch = Some(run.branch());
            item.pr = Some(PrRef {
                number,
                draft: run.options.draft_pr,
                checks,
            });
            item.status = TicketStatus::Review;
            item.agent_note = Some(format!("Vom Agenten umgesetzt · {tests}"));
            item.reason = ReasonKind::AgentFinished;
            item.reason_text = format!("Agent fertig · Pull Request #{number} erstellt");
            item.time = "gerade eben".into();
            item.read = false;
        }
        self.reviews.push(Review {
            number,
            title: run.ticket_title.clone(),
            description: format!(
                "Vom Agenten für {} umgesetzt · {tests}. Der Agent führt keinen Merge aus.",
                run.ticket_key
            ),
            author: Person::agent(),
            branch: run.branch(),
            base: run.options.base_branch.clone(),
            ticket_key: Some(run.ticket_key.clone()),
            checks,
            files: demo::agent_changes(&run.ticket_key),
            agent_reviewed: false,
            suggestions: Vec::new(),
            line_comments: Vec::new(),
            verdict: Verdict::Comment,
            sent: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::STEPS;

    fn ws() -> Workspace {
        demo::workspace()
    }

    /// Optionen ohne Plan-Freigabe: der Agent läuft sofort.
    fn direct() -> AgentOptions {
        AgentOptions {
            plan_first: false,
            ..Default::default()
        }
    }

    fn finish_agents(w: &mut Workspace) -> Vec<String> {
        (0..STEPS).flat_map(|_| w.advance_agents()).collect()
    }

    #[test]
    fn demo_data_matches_the_design() {
        let w = ws();
        assert_eq!(w.items.len(), 5);
        assert_eq!(w.unread(Filter::All), 5);
        assert_eq!(
            (w.visible(Filter::Mine).len(), w.visible(Filter::Team).len()),
            (3, 2)
        );
        assert_eq!(w.review(87).unwrap().suggestions.len(), 3);
        assert_eq!(w.open_reviews(), 3);
        assert_eq!(w.running_agents(), 0);
        // Jeder Pull Request an einem Eintrag hat ein Review.
        for pr in w.items.iter().filter_map(|i| i.pr.as_ref()) {
            assert!(w.review(pr.number).is_some(), "#{}", pr.number);
        }
    }

    #[test]
    fn done_items_stop_counting() {
        let mut w = ws();
        w.mark_done("bau142");
        assert_eq!(w.unread(Filter::All), 4);
        assert_eq!(w.unread(Filter::Mine), 2);
        assert_eq!(w.unread(Filter::Team), 2);
    }

    #[test]
    fn only_tickets_have_a_status() {
        let mut w = ws();
        assert!(w.set_status("bau150", TicketStatus::Done));
        assert_eq!(w.item("bau150").unwrap().status, TicketStatus::Done);
        assert!(!w.set_status("pr87", TicketStatus::Done));
        assert!(!w.set_status("gibt-es-nicht", TicketStatus::Done));
    }

    #[test]
    fn branch_is_named_after_key_and_title() {
        let mut w = ws();
        assert_eq!(
            w.create_branch("bau150").as_deref(),
            Some("feature/BAU-150-sheet-detents-mittel-und")
        );
        assert_eq!(w.create_branch("bau150"), None, "es gibt schon einen");
        assert_eq!(w.create_branch("pr87"), None);
    }

    #[test]
    fn replies_are_trimmed_and_mine() {
        let mut w = ws();
        assert!(!w.reply("bau142", "  \n "));
        assert!(w.reply("bau142", "  Mache ich.  "));
        let last = w.item("bau142").unwrap().comments.last().unwrap();
        assert_eq!(
            (last.text.as_str(), last.own, last.author.name.as_str()),
            ("Mache ich.", true, "Du")
        );
    }

    #[test]
    fn sending_a_review_closes_the_inbox_item() {
        let mut w = ws();
        assert_eq!(w.send_review(87, ""), Err(SendError::Undecided(3)));
        assert_eq!(w.send_review(1, ""), Err(SendError::UnknownReview));
        w.decide(87, "a", Some(Decision::Approved));
        w.decide(87, "b", Some(Decision::Declined));
        w.decide(87, "c", Some(Decision::Approved));
        w.set_verdict(87, Verdict::RequestChanges);
        assert_eq!(w.send_review(87, " Bitte Fokus prüfen. "), Ok(2));

        let item = w.item("pr87").unwrap();
        let comment = item.comments.last().unwrap();
        assert_eq!(
            comment.text,
            "Review gesendet: Änderungen · 2 Kommentare\nBitte Fokus prüfen."
        );
        assert!(comment.own && item.read);
        assert_eq!(item.agent_note, None);
        assert_eq!(w.open_reviews(), 2);
    }

    #[test]
    fn own_line_comments_go_out_with_the_review() {
        let mut w = ws();
        let file = w.review(87).unwrap().files[0].name.clone();
        assert_eq!(w.add_line_comment(1, &file, 0, "x"), None);
        let id = w.add_line_comment(87, &file, 0, "Warum hier?").unwrap();
        for s in ["a", "b", "c"] {
            w.decide(87, s, Some(Decision::Declined));
        }
        // Eigene Kommentare allein zählen mit, auch wenn alle Vorschläge abgelehnt sind.
        assert_eq!(w.send_review(87, ""), Ok(1));
        assert_eq!(
            w.item("pr87").unwrap().comments.last().unwrap().text,
            "Review gesendet: Kommentieren · 1 Kommentar"
        );
        assert!(w.review(87).unwrap().sent);
        assert!(w.edit_line_comment(87, &id, "Anders"));
        assert!(!w.review(87).unwrap().sent);
        assert!(w.delete_line_comment(87, &id));
        assert!(!w.delete_line_comment(1, &id));
    }

    #[test]
    fn agent_review_prepares_suggestions() {
        let mut w = ws();
        assert!(!w.review(85).unwrap().agent_reviewed);
        let count = w.request_agent_review(85);
        assert!(count >= 1);
        assert!(w.review(85).unwrap().agent_reviewed);
        assert!(
            w.item("pr85")
                .unwrap()
                .agent_note
                .as_ref()
                .unwrap()
                .contains("vom Agenten")
        );
        assert_eq!(w.send_review(85, ""), Err(SendError::Undecided(count)));
    }

    #[test]
    fn an_agent_turns_a_ticket_into_a_pull_request() {
        let mut w = ws();
        assert_eq!(w.next_pr_number(), 92);
        w.start_agent("BAU-142", direct()).unwrap();
        assert_eq!(w.running_agents(), 1);
        assert_eq!(w.item("bau142").unwrap().status, TicketStatus::InProgress);
        assert_eq!(
            w.start_agent("BAU-142", direct()),
            Err(AgentError::AlreadyRunning)
        );

        assert_eq!(finish_agents(&mut w), vec!["BAU-142".to_string()]);
        assert_eq!(w.running_agents(), 0);

        let item = w.item("bau142").unwrap();
        assert_eq!(
            item.branch.as_deref(),
            Some("agent/BAU-142-toggle-fokus-ring-fehlt")
        );
        assert_eq!(item.pr.as_ref().unwrap().label(), "#92 · Entwurf");
        assert_eq!(item.status, TicketStatus::Review);
        assert_eq!(item.reason, ReasonKind::AgentFinished);
        assert!(!item.read, "ein fertiger Lauf braucht Aufmerksamkeit");

        let review = w.review(92).unwrap();
        assert!(review.author.is_agent());
        assert_eq!(review.files.len(), 2);
        assert_eq!(review.ticket_key.as_deref(), Some("BAU-142"));
        assert_eq!(w.next_pr_number(), 93);
    }

    #[test]
    fn two_agents_get_different_pull_requests() {
        let mut w = ws();
        w.start_agent("BAU-142", direct()).unwrap();
        w.start_agent("BAU-150", direct()).unwrap();
        assert_eq!(w.running_agents(), 2);
        let mut finished = finish_agents(&mut w);
        finished.sort();
        assert_eq!(finished, ["BAU-142", "BAU-150"]);
        assert_eq!(w.item("bau142").unwrap().pr.as_ref().unwrap().number, 92);
        assert_eq!(w.item("bau150").unwrap().pr.as_ref().unwrap().number, 93);
    }

    #[test]
    fn options_decide_draft_and_checks() {
        let mut w = ws();
        let options = AgentOptions {
            run_tests: false,
            draft_pr: false,
            ..direct()
        };
        w.start_agent("BAU-150", options).unwrap();
        finish_agents(&mut w);
        let item = w.item("bau150").unwrap();
        assert_eq!(
            item.pr,
            Some(PrRef {
                number: 92,
                draft: false,
                checks: Checks::Running
            })
        );
        assert_eq!(
            item.agent_note.as_deref(),
            Some("Vom Agenten umgesetzt · Tests übersprungen")
        );
    }

    #[test]
    fn a_cancelled_agent_leaves_no_traces() {
        let mut w = ws();
        w.start_agent("BAU-142", direct()).unwrap();
        w.advance_agents();
        w.cancel_agent("BAU-142");
        assert_eq!(w.running_agents(), 0);
        assert!(finish_agents(&mut w).is_empty());
        let item = w.item("bau142").unwrap();
        assert_eq!(
            (
                item.branch.clone(),
                item.pr.clone(),
                item.agent_note.clone()
            ),
            (None, None, None)
        );
        assert!(w.review(92).is_none());
        // Ein fertiger Lauf lässt sich nicht mehr abbrechen.
        w.cancel_agent("BAU-138");
        assert!(w.run("BAU-138").unwrap().is_done());
    }

    #[test]
    fn a_plan_is_shown_before_anything_happens() {
        let mut w = ws();
        w.start_agent("BAU-142", AgentOptions::default()).unwrap();
        assert_eq!((w.running_agents(), w.waiting_agents()), (0, 1));
        assert_eq!(
            w.start_agent("BAU-142", AgentOptions::default()),
            Err(AgentError::AlreadyRunning)
        );
        // Der Takt bringt keinen Fortschritt ohne Freigabe.
        assert!(finish_agents(&mut w).is_empty());
        let run = w.run("BAU-142").unwrap();
        assert!(run.is_awaiting());
        assert_eq!(run.completed, 0);
        // Weder Branch noch Pull Request noch neuer Status.
        let item = w.item("bau142").unwrap();
        assert_eq!((item.branch.clone(), item.pr.clone()), (None, None));
        assert_eq!(item.status, ws().item("bau142").unwrap().status);
        assert_eq!(
            item.agent_note.as_deref(),
            Some("Der Agent wartet auf die Freigabe seines Plans")
        );
        assert!(w.review(92).is_none());
    }

    #[test]
    fn approving_continues_to_the_pull_request() {
        let mut w = ws();
        w.start_agent("BAU-142", AgentOptions::default()).unwrap();
        assert!(w.approve_plan("BAU-142"));
        assert!(!w.approve_plan("BAU-142"), "nur einmal freigeben");
        assert_eq!((w.running_agents(), w.waiting_agents()), (1, 0));
        assert_eq!(finish_agents(&mut w), vec!["BAU-142".to_string()]);
        assert_eq!(w.item("bau142").unwrap().pr.as_ref().unwrap().number, 92);
        assert!(w.review(92).is_some());
    }

    #[test]
    fn rejecting_a_plan_leaves_no_traces() {
        let mut w = ws();
        let before = w.clone();
        w.start_agent("BAU-142", AgentOptions::default()).unwrap();
        assert!(w.reject_plan("BAU-142"));
        assert!(!w.reject_plan("BAU-142"));
        assert_eq!((w.running_agents(), w.waiting_agents()), (0, 0));
        assert!(finish_agents(&mut w).is_empty());
        assert_eq!(w.item("bau142"), before.item("bau142"));
        assert!(w.review(92).is_none());
        assert!(w.run("BAU-142").unwrap().is_idle());
        // Danach lässt sich neu starten.
        assert!(w.start_agent("BAU-142", AgentOptions::default()).is_ok());
    }

    #[test]
    fn a_waiting_plan_can_be_cancelled_and_revised() {
        let mut w = ws();
        w.start_agent("BAU-150", AgentOptions::default()).unwrap();
        assert!(!w.revise_plan("BAU-150", "  "), "leer zählt nicht");
        assert!(w.revise_plan("BAU-150", "Auch die Doku anpassen"));
        assert!(w.run("BAU-150").unwrap().is_awaiting());
        w.cancel_agent("BAU-150");
        assert!(w.run("BAU-150").unwrap().is_idle());
        assert_eq!(w.item("bau150").unwrap().agent_note, None);
        assert!(!w.revise_plan("BAU-150", "zu spät"));
    }

    #[test]
    fn agents_only_work_on_tickets() {
        let mut w = ws();
        assert_eq!(
            w.start_agent("#87", direct()),
            Err(AgentError::UnknownTicket)
        );
        assert_eq!(
            w.start_agent("NOPE-1", direct()),
            Err(AgentError::UnknownTicket)
        );
        assert!(w.run("#87").is_none());
        let fresh = w.run("BAU-150").unwrap();
        assert!(fresh.is_idle());
        assert_eq!(fresh.files.len(), 2);
    }
}
