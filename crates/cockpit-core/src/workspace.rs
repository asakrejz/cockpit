//! Der Workspace: alle Daten des Cockpits und die Aktionen der Oberfläche.

use serde::{Deserialize, Serialize};

use crate::action::LogEntry;
use crate::agent::{AgentOptions, AgentRun, RunKind, Tick};
use crate::attention::{self, Hit, Rule, Rules, Snooze};
use crate::demo;
use crate::diagnosis::diagnose;
use crate::model::*;
use crate::notify::NotifySettings;
use crate::review::{CHECK_STEPS, Decision, MergeBlocker, MergeMethod, Review, SendError, Verdict};
use crate::text::slug;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentError {
    UnknownTicket,
    AlreadyRunning,
    /// Den Pull Request gibt es nicht.
    UnknownReview,
    /// Der Pull Request ist gemergt und bleibt, wie er ist.
    Merged,
    /// Es sind keine offenen Kommentare ausgewählt (oder einer ist unbekannt oder erledigt).
    NoComments,
    /// Untersuchen lässt sich nur ein roter Check.
    CheckNotFailed,
}

/// Kurze Kennung für einen Commit der Demo: stabil, aber nicht echt.
fn short_sha(seed: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:07x}", hash & 0x0fff_ffff)
}

/// Warum sich Ticket und Pull Request nicht verknüpfen lassen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    /// Es gibt kein Review (keinen Pull Request) mit dieser Nummer.
    UnknownPr(u32),
    /// Das Ticket hängt schon an diesem Pull Request.
    Same,
    /// Das Ticket hat schon einen anderen Pull Request (Nummer).
    TicketHasPr(u32),
    /// Der Pull Request gehört schon zu einem anderen Ticket (Schlüssel).
    PrHasTicket(String),
}

/// Ein Statuswechsel, den der Pull Request ausgelöst hat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusChange {
    /// Interne ID des Tickets.
    pub item: String,
    pub key: String,
    pub pr: u32,
    pub from: TicketStatus,
    pub to: TicketStatus,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Workspace {
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub reviews: Vec<Review>,
    #[serde(default)]
    pub runs: Vec<AgentRun>,
    /// Protokoll aller Aktionen, älteste zuerst. Siehe [`crate::action`].
    #[serde(default)]
    pub log: Vec<LogEntry>,
    /// Benachrichtigungen je Grund und Kanal, dazu die Ruhezeiten.
    #[serde(default)]
    pub notifications: NotifySettings,
    /// Welche Aufmerksamkeits-Regeln gelten. Siehe [`crate::attention`].
    #[serde(default)]
    pub rules: Rules,
    /// Zurückgestellte Einträge.
    #[serde(default)]
    pub snoozed: Vec<Snooze>,
    /// Von Hand gelöste Verknüpfungen `(Pull Request, Ticket-Schlüssel)`: Die Automatik
    /// setzt sie nicht wieder.
    #[serde(default)]
    pub unlinked: Vec<(u32, String)>,
    /// Ist der Haken „Status folgt dem Pull Request“ aus? Gespeichert wird die Abschaltung:
    /// So gilt die Automatik in alten Ständen sofort.
    #[serde(default)]
    pub pr_status_off: bool,
    /// Zu welchem Stand des Pull Requests der Status zuletzt nachgeführt wurde
    /// `(Ticket-Schlüssel, Status)`. Eine Änderung von Hand bleibt so lange stehen, bis sich
    /// der Pull Request wieder bewegt.
    #[serde(default)]
    pub status_seen: Vec<(String, TicketStatus)>,
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
    ///
    /// Es zählen die Regeln: Ein Eintrag steht nur drin, wenn mindestens eine
    /// eingeschaltete Regel zutrifft und er nicht zurückgestellt ist. Der Filter
    /// „Zurückgestellt“ zeigt stattdessen die zurückgestellten Einträge.
    pub fn search(
        &self,
        filter: Filter,
        source: SourceFilter,
        query: &str,
        now_ms: u64,
    ) -> Vec<&Item> {
        self.attention(filter, now_ms)
            .into_iter()
            .filter(|i| source.matches(i.source) && crate::search::matches_query(i, query))
            .collect()
    }

    /// Einträge, die für diesen Filter Aufmerksamkeit brauchen (oder, bei
    /// [`Filter::Snoozed`], gerade zurückgestellt sind), in Anzeigereihenfolge.
    pub fn attention(&self, filter: Filter, now_ms: u64) -> Vec<&Item> {
        self.visible(filter)
            .into_iter()
            .filter(|i| match filter {
                Filter::Snoozed => self.is_snoozed(&i.id, now_ms),
                _ => !self.is_snoozed(&i.id, now_ms) && self.needs_attention(i, now_ms),
            })
            .collect()
    }

    /// Treffer der eingeschalteten Regeln für einen Eintrag, wichtigster zuerst.
    pub fn hits(&self, item: &Item, now_ms: u64) -> Vec<Hit> {
        attention::hits(item, &self.rules, now_ms)
    }

    pub fn needs_attention(&self, item: &Item, now_ms: u64) -> bool {
        !self.hits(item, now_ms).is_empty()
    }

    /// Einträge, die noch Aufmerksamkeit brauchen. Bei [`Filter::Snoozed`]: wie viele zurückgestellt sind.
    pub fn unread(&self, filter: Filter, now_ms: u64) -> usize {
        self.attention(filter, now_ms)
            .iter()
            .filter(|i| filter == Filter::Snoozed || !i.read)
            .count()
    }

    pub fn snooze_of(&self, id: &str) -> Option<&Snooze> {
        self.snoozed.iter().find(|s| s.item == id)
    }

    /// Ist der Eintrag gerade zurückgestellt?
    pub fn is_snoozed(&self, id: &str, now_ms: u64) -> bool {
        match (self.snooze_of(id), self.item(id)) {
            (Some(snooze), Some(item)) => snooze.holds(item, now_ms),
            _ => false,
        }
    }

    /// IDs der Einträge, deren Zurückstellung abgelaufen oder durch eine Änderung
    /// aufgehoben ist (und die `wake` deshalb noch entfernen muss).
    pub fn expired_snoozes(&self, now_ms: u64) -> Vec<String> {
        self.snoozed
            .iter()
            .filter(|s| !self.is_snoozed(&s.item, now_ms))
            .map(|s| s.item.clone())
            .collect()
    }

    /// Stellt einen Eintrag zurück (`until_ms`: bis dann; `None`: bis sich etwas ändert).
    /// Eine frühere Zurückstellung wird ersetzt.
    pub fn snooze(&mut self, id: &str, until_ms: Option<u64>) -> bool {
        let Some(item) = self.item(id) else {
            return false;
        };
        let snooze = Snooze::new(item, until_ms);
        self.snoozed.retain(|s| s.item != id);
        self.snoozed.push(snooze);
        true
    }

    /// Holt einen Eintrag zurück. `false`, wenn er nicht zurückgestellt war.
    pub fn unsnooze(&mut self, id: &str) -> bool {
        let before = self.snoozed.len();
        self.snoozed.retain(|s| s.item != id);
        self.snoozed.len() != before
    }

    /// Entfernt abgelaufene Zurückstellungen. Gibt die IDs der zurückgekehrten Einträge zurück.
    pub fn wake(&mut self, now_ms: u64) -> Vec<String> {
        let back = self.expired_snoozes(now_ms);
        self.snoozed.retain(|s| !back.contains(&s.item));
        back
    }

    /// Schaltet eine Regel ein oder aus. `false`, wenn sich nichts ändert.
    pub fn set_rule(&mut self, rule: Rule, enabled: bool) -> bool {
        self.rules.set(rule, enabled)
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

    /// Agenten, die auf den Nutzer warten: auf die Freigabe eines Plans, eine Antwort oder
    /// die Entscheidung über einen Fix.
    pub fn waiting_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_waiting_for_you()).count()
    }

    /// Agenten, die eine Frage gestellt haben.
    pub fn asking_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_asking()).count()
    }

    /// Pausierte Agenten.
    pub fn paused_agents(&self) -> usize {
        self.runs.iter().filter(|r| r.is_paused()).count()
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

    // ---------- Tickets bearbeiten ----------

    /// Ändert Titel und Beschreibung eines Tickets. Der Titel darf nicht leer sein.
    pub fn edit_ticket(&mut self, id: &str, title: &str, description: &str) -> bool {
        let title = title.trim();
        match self.item_mut(id) {
            Some(item) if item.is_ticket() && !title.is_empty() => {
                item.title = title.to_string();
                item.description = description.trim().to_string();
                true
            }
            _ => false,
        }
    }

    /// Personen, denen sich ein Ticket zuweisen lässt: erst du, dann alle anderen, die in der
    /// Inbox, als Autor eines Pull Requests oder in den Kommentaren auftauchen. Der Agent bekommt Tickets über
    /// seinen Lauf, nicht über die Zuweisung.
    pub fn people(&self) -> Vec<Person> {
        let mut list = vec![Person::me()];
        let owners = self.items.iter().map(|i| &i.owner);
        let authors = self.reviews.iter().map(|r| &r.author);
        let commenters = self
            .items
            .iter()
            .flat_map(|i| i.comments.iter().map(|c| &c.author));
        for person in owners.chain(authors).chain(commenters) {
            if !person.is_agent() && !person.name.is_empty() && !list.contains(person) {
                list.push(person.clone());
            }
        }
        list
    }

    /// Weist ein Ticket jemandem zu. Wem es gehört, entscheidet auch, unter „Ich“ oder „Team“ es
    /// steht. Ein Ticket, das neu bei dir landet, gilt wieder als ungelesen und löst die Regel
    /// „Zugewiesen“ aus; geht es an jemand anderen, endet sie.
    pub fn assign(&mut self, id: &str, person: &Person) -> bool {
        let me = Person::me();
        let Some(item) = self.item_mut(id).filter(|i| i.is_ticket()) else {
            return false;
        };
        item.owner = person.clone();
        item.scope = if *person == me {
            Scope::Mine
        } else {
            Scope::Team
        };
        if *person == me {
            item.read = false;
            item.reason = ReasonKind::Assigned;
            item.reason_text = "Dir zugewiesen".into();
            item.time = "gerade eben".into();
        }
        true
    }

    pub fn set_priority(&mut self, id: &str, priority: Priority) -> bool {
        match self.item_mut(id) {
            Some(item) if item.is_ticket() => {
                item.priority = priority;
                true
            }
            _ => false,
        }
    }

    // ---------- Ticket und Pull Request verknüpfen ----------

    /// Das Ticket, das zu diesem Pull Request gehört (über das Review).
    pub fn ticket_of_pr(&self, number: u32) -> Option<&Item> {
        let key = self.review(number)?.ticket_key.as_deref()?;
        self.item_by_key(key).filter(|i| i.is_ticket())
    }

    /// Die Kopie des Pull Requests für ein Ticket: der Eintrag aus der Inbox, sonst aus dem Review.
    fn pr_ref_of(&self, number: u32) -> Option<PrRef> {
        let from_item = self
            .items
            .iter()
            .filter(|i| i.kind == Kind::PullRequest)
            .find_map(|i| i.pr.clone().filter(|p| p.number == number));
        from_item.or_else(|| {
            self.review(number).map(|r| PrRef {
                number,
                draft: false,
                checks: r.checks,
                changes_requested: false,
                merged: r.merged.is_some(),
                review_requested: false,
            })
        })
    }

    /// Verknüpft ein Ticket mit einem Pull Request. Das Ticket bekommt den Pull Request (und,
    /// wenn es noch keinen hat, den Branch), das Review das Ticket.
    pub fn link_pr(&mut self, ticket_id: &str, number: u32) -> Result<(), LinkError> {
        let key = self
            .item(ticket_id)
            .map(|i| i.key.clone())
            .unwrap_or_default();
        let review = self.review(number).ok_or(LinkError::UnknownPr(number))?;
        let branch = review.branch.clone();
        if let Some(other) = self.ticket_of_pr(number).map(|t| t.key.clone()) {
            return Err(if other == key {
                LinkError::Same
            } else {
                LinkError::PrHasTicket(other)
            });
        }
        let current = self
            .item(ticket_id)
            .and_then(|i| i.pr.as_ref().map(|p| p.number));
        if let Some(n) = current {
            // Zeigt das Ticket schon auf diesen Pull Request, fehlt nur die Gegenseite.
            if n != number
                && self
                    .review(n)
                    .is_some_and(|r| r.ticket_key.as_deref() == Some(&key))
            {
                return Err(LinkError::TicketHasPr(n));
            }
        }
        let pr = self.pr_ref_of(number);
        if let Some(item) = self.item_mut(ticket_id) {
            item.pr = pr;
            if item.branch.is_none() && !branch.is_empty() {
                item.branch = Some(branch);
            }
        }
        if let Some(review) = self.review_mut(number) {
            review.ticket_key = Some(key.clone());
        }
        self.unlinked.retain(|(n, k)| !(*n == number && *k == key));
        Ok(())
    }

    /// Löst die Verknüpfung eines Tickets. Gibt die Nummer des Pull Requests zurück.
    /// Die Automatik verknüpft dasselbe Paar nicht wieder.
    pub fn unlink_pr(&mut self, ticket_id: &str) -> Option<u32> {
        let item = self.item(ticket_id).filter(|i| i.is_ticket())?;
        let (key, number) = (item.key.clone(), item.pr.as_ref()?.number);
        let review_branch = self.review(number).map(|r| r.branch.clone());
        if let Some(review) = self
            .review_mut(number)
            .filter(|r| r.ticket_key.as_deref() == Some(&key))
        {
            review.ticket_key = None;
        }
        if let Some(item) = self.item_mut(ticket_id) {
            item.pr = None;
            if item.branch == review_branch {
                item.branch = None;
            }
        }
        self.status_seen.retain(|(k, _)| *k != key);
        if !self.unlinked.contains(&(number, key.clone())) {
            self.unlinked.push((number, key));
        }
        Some(number)
    }

    /// Welche Pull Requests die Automatik jetzt verknüpfen würde: `(Ticket-ID, Pull Request)`.
    /// Ein Pull Request kommt in Frage, solange kein vorhandenes Ticket zu ihm gehört und
    /// Branch oder Titel den Schlüssel eines Tickets ohne Pull Request nennen. Von Hand
    /// gelöste Paare bleiben gelöst.
    pub fn auto_links(&self) -> Vec<(String, u32)> {
        let mut taken: Vec<&str> = Vec::new();
        let mut links = Vec::new();
        for review in self
            .reviews
            .iter()
            .filter(|r| self.ticket_of_pr(r.number).is_none())
        {
            let free = self.items.iter().filter(|i| {
                i.is_ticket()
                    && i.pr.is_none()
                    && !taken.contains(&i.key.as_str())
                    && !self.unlinked.contains(&(review.number, i.key.clone()))
            });
            let known = free.clone().map(|i| i.key.as_str());
            if let Some(key) = crate::link::find_ticket(&[&review.branch, &review.title], known)
                && let Some(ticket) = free.clone().find(|i| i.key == key)
            {
                taken.push(&ticket.key);
                links.push((ticket.id.clone(), review.number));
            }
        }
        links
    }

    /// Verknüpft, was `auto_links` findet. Gibt `(Ticket-ID, Schlüssel, Pull Request)` zurück.
    pub fn auto_link(&mut self) -> Vec<(String, String, u32)> {
        let mut done = Vec::new();
        for (id, number) in self.auto_links() {
            if self.link_pr(&id, number).is_ok() {
                let key = self.item(&id).map(|i| i.key.clone()).unwrap_or_default();
                done.push((id, key, number));
            }
        }
        done
    }

    // ---------- Status folgt dem Pull Request ----------

    /// Verknüpfte Tickets mit dem Status, den ihr Pull Request nahelegt: `(Index, Pull Request, Ziel)`.
    fn linked_targets(&self) -> Vec<(usize, u32, TicketStatus)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.is_ticket())
            .filter_map(|(at, i)| {
                let pr = i.pr.as_ref()?;
                let is_linked = self
                    .review(pr.number)
                    .is_some_and(|r| r.ticket_key.as_deref() == Some(i.key.as_str()));
                is_linked.then(|| (at, pr.number, pr.target_status()))
            })
            .collect()
    }

    /// Hat sich ein Pull Request seit dem letzten Nachführen bewegt? (Ohne etwas zu ändern.)
    pub fn status_pending(&self) -> bool {
        !self.pr_status_off
            && self.linked_targets().into_iter().any(|(at, _, target)| {
                let key = &self.items[at].key;
                self.status_seen
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, t)| *t)
                    != Some(target)
            })
    }

    /// Merkt sich, dass der Status zum aktuellen Stand des Pull Requests passt, ohne ihn zu ändern.
    fn mark_status_seen(&mut self, key: &str, target: TicketStatus) {
        match self.status_seen.iter_mut().find(|(k, _)| k == key) {
            Some((_, seen)) => *seen = target,
            None => self.status_seen.push((key.to_string(), target)),
        }
    }

    /// Führt den Status verknüpfter Tickets nach: Pull Request gemergt → Fertig, Review
    /// angefragt → Review, sonst → In Arbeit. Nur, wenn sich der Stand des Pull Requests seit
    /// dem letzten Mal geändert hat; ein Status, den du danach von Hand setzt, bleibt so lange
    /// stehen, bis sich der Pull Request wieder bewegt. Bei ausgeschalteter Automatik tut es nichts.
    pub fn follow_status(&mut self) -> Vec<StatusChange> {
        if self.pr_status_off {
            return Vec::new();
        }
        let mut changes = Vec::new();
        for (at, pr, target) in self.linked_targets() {
            let key = self.items[at].key.clone();
            let seen = self.status_seen.iter().find(|(k, _)| *k == key);
            if seen.is_some_and(|(_, t)| *t == target) {
                continue;
            }
            self.mark_status_seen(&key, target);
            let item = &mut self.items[at];
            if item.status == target {
                continue;
            }
            changes.push(StatusChange {
                item: item.id.clone(),
                key,
                pr,
                from: item.status,
                to: target,
            });
            item.status = target;
            if target == TicketStatus::Done {
                item.read = true;
                item.agent_note = None;
            }
        }
        changes
    }

    /// Schaltet „Status folgt dem Pull Request“ ein oder aus. `false`, wenn sich nichts ändert.
    pub fn set_status_follows_pr(&mut self, enabled: bool) -> bool {
        if self.pr_status_off != enabled {
            return false;
        }
        self.pr_status_off = !enabled;
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
        suggestion: Option<&str>,
    ) -> Option<String> {
        self.review_mut(number)?
            .add_line_comment(file, line, text, suggestion)
    }

    pub fn edit_line_comment(
        &mut self,
        number: u32,
        comment: &str,
        text: &str,
        suggestion: Option<&str>,
    ) -> bool {
        self.review_mut(number)
            .is_some_and(|r| r.edit_line_comment(comment, text, suggestion))
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
            // Das Review ist abgegeben; die Anfrage ist beantwortet.
            if item.reason == ReasonKind::ReviewRequested {
                item.reason = ReasonKind::Comment;
            }
        }
        Ok(sent)
    }

    // ---------- Checks ----------

    /// Checks, die noch laufen (über alle Pull Requests).
    pub fn running_checks(&self) -> usize {
        self.reviews.iter().map(Review::running_checks).sum()
    }

    /// Schreibt den Gesamtstand der Checks in die Einträge der Inbox.
    fn sync_pr_checks(&mut self, number: u32) {
        let Some(checks) = self.review(number).map(|r| r.checks) else {
            return;
        };
        for item in self
            .items
            .iter_mut()
            .filter(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
        {
            if let Some(pr) = item.pr.as_mut() {
                pr.checks = checks;
            }
            // Wieder grün: Der Eintrag hängt nicht mehr an den Checks.
            if checks == Checks::Passed && item.reason == ReasonKind::ChecksFailed {
                item.reason = ReasonKind::ReviewRequested;
                item.reason_text = "Checks wieder bestanden".into();
            }
        }
    }

    /// Startet einen fehlgeschlagenen Check neu.
    pub fn restart_check(&mut self, number: u32, check: &str) -> bool {
        let restarted = self
            .review_mut(number)
            .is_some_and(|r| r.restart_check(check));
        if restarted {
            self.sync_pr_checks(number);
        }
        restarted
    }

    /// Der Takt der Checks: jeder laufende kommt einen Schritt weiter. Gibt
    /// `(Pull Request, Check, Ergebnis)` für jeden zurück, der dabei fertig geworden ist.
    pub fn advance_checks(&mut self) -> Vec<(u32, String, Checks)> {
        let mut finished = Vec::new();
        for review in &mut self.reviews {
            for name in review.advance_checks() {
                let status = review
                    .check_runs
                    .iter()
                    .find(|c| c.name == name)
                    .map_or(Checks::Failed, |c| c.status);
                finished.push((review.number, name, status));
            }
        }
        let numbers: Vec<u32> = finished.iter().map(|(n, ..)| *n).collect();
        for n in numbers {
            self.sync_pr_checks(n);
        }
        finished
    }

    // ---------- Mergen ----------

    /// Was das Mergen von Pull Request `number` verhindert. Unbekannt = leer.
    pub fn merge_blockers(&self, number: u32) -> Vec<MergeBlocker> {
        let Some(review) = self.review(number) else {
            return Vec::new();
        };
        let mut blockers = review.merge_blockers();
        let draft = self
            .items
            .iter()
            .any(|i| i.pr.as_ref().is_some_and(|p| p.number == number && p.draft));
        if draft && review.merged.is_none() {
            blockers.insert(0, MergeBlocker::Draft);
        }
        blockers
    }

    /// Mergt den Pull Request. Der Eintrag in der Inbox zeigt „Gemergt“. Das verknüpfte Ticket
    /// folgt über [`Workspace::follow_status`] (sofern das eingeschaltet ist); gibt dessen
    /// Statuswechsel zurück.
    pub fn merge_pr(
        &mut self,
        number: u32,
        method: MergeMethod,
    ) -> Result<Vec<StatusChange>, Vec<MergeBlocker>> {
        let blockers = self.merge_blockers(number);
        if !blockers.is_empty() {
            return Err(blockers);
        }
        let Some(review) = self.review_mut(number) else {
            return Err(Vec::new());
        };
        review.merged = Some(method);
        let ticket = review.ticket_key.clone();

        for item in self
            .items
            .iter_mut()
            .filter(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
        {
            if let Some(pr) = item.pr.as_mut() {
                pr.merged = true;
                pr.draft = false;
            }
            item.read = true;
            item.agent_note = None;
            if item.kind == Kind::PullRequest {
                item.reason_text = format!("Gemergt ({})", method.label());
            }
        }
        // Ein verknüpftes Ticket ohne eigene Kopie des Pull Requests bekommt sie jetzt.
        if let Some(id) = ticket
            .and_then(|key| self.item_by_key(&key))
            .filter(|i| i.is_ticket() && i.pr.is_none())
            .map(|i| i.id.clone())
        {
            let pr = self.pr_ref_of(number);
            if let (Some(item), Some(pr)) = (self.item_mut(&id), pr) {
                item.pr = Some(pr);
            }
        }
        Ok(self.follow_status())
    }

    /// Fragt ein Review an: Der Pull Request gilt als „Review angefragt“. `false`, wenn er es schon ist.
    pub fn request_review(&mut self, number: u32) -> bool {
        let mut changed = false;
        for pr in self
            .items
            .iter_mut()
            .filter_map(|i| i.pr.as_mut())
            .filter(|p| p.number == number && !p.review_requested)
        {
            pr.review_requested = true;
            changed = true;
        }
        changed
    }

    // ---------- Agenten ----------

    /// Der Lauf zu einem Ticket. Gibt es noch keinen, kommt ein startbereiter zurück.
    pub fn run(&self, ticket_key: &str) -> Option<AgentRun> {
        if let Some(run) = self.runs.iter().find(|r| r.key() == ticket_key) {
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
        // Folgeaufträge starten über das Review, nicht über ein Ticket.
        if run.is_followup() {
            return Err(AgentError::UnknownTicket);
        }
        if run.is_active() {
            return Err(AgentError::AlreadyRunning);
        }
        run.start(options, self.next_pr_number());
        let waiting = run.is_awaiting();
        match self.runs.iter_mut().find(|r| r.key() == ticket_key) {
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

    fn active_run_mut(&mut self, key: &str) -> Option<&mut AgentRun> {
        self.runs
            .iter_mut()
            .find(|r| r.key() == key && r.is_active())
    }

    /// Der Eintrag, an dem der Lauf hängt: bei Folgeaufträgen der mit dem Pull Request.
    fn run_item_mut(&mut self, key: &str) -> Option<&mut Item> {
        let item_key = self
            .runs
            .iter()
            .find(|r| r.key() == key)
            .map(|r| r.ticket_key.clone())?;
        self.items.iter_mut().find(|i| i.key == item_key)
    }

    fn clear_agent_note(&mut self, key: &str) {
        if let Some(item) = self.run_item_mut(key) {
            item.agent_note = None;
        }
    }

    /// Ein Folgeauftrag, der nicht mehr läuft, hinterlässt keinen Lauf. (Beim Ticket
    /// bleibt der bereite Lauf stehen, wie bisher.)
    fn drop_idle_followups(&mut self) {
        self.runs.retain(|r| !(r.is_followup() && r.is_idle()));
    }

    /// Bricht einen wartenden oder laufenden Agenten ab.
    pub fn cancel_agent(&mut self, ticket_key: &str) {
        let Some(run) = self.active_run_mut(ticket_key) else {
            return;
        };
        run.reset();
        self.clear_agent_note(ticket_key);
        self.drop_idle_followups();
    }

    /// Gibt den Plan frei: Der Agent legt los, das Ticket geht auf „In Arbeit“.
    pub fn approve_plan(&mut self, ticket_key: &str) -> bool {
        let followup = self
            .runs
            .iter()
            .any(|r| r.key() == ticket_key && r.is_followup());
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::approve)
        {
            return false;
        }
        if let Some(item) = self.run_item_mut(ticket_key) {
            // Ein Folgeauftrag ändert den Status des Tickets nicht.
            if !followup {
                item.status = TicketStatus::InProgress;
            }
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
        self.drop_idle_followups();
        true
    }

    /// Fordert mit einer Anmerkung einen neuen Plan an.
    pub fn revise_plan(&mut self, ticket_key: &str, note: &str) -> bool {
        self.active_run_mut(ticket_key)
            .is_some_and(|r| r.revise(note))
    }

    fn set_agent_note(&mut self, key: &str, note: &str) {
        if let Some(item) = self.run_item_mut(key) {
            item.agent_note = Some(note.into());
        }
    }

    /// Beantwortet die Frage des Agenten; er arbeitet danach weiter.
    pub fn answer_agent(&mut self, ticket_key: &str, text: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(|r| r.answer(text))
        {
            return false;
        }
        self.set_agent_note(ticket_key, "Der Agent arbeitet an der Umsetzung");
        true
    }

    /// Schickt dem laufenden Agenten einen Hinweis für den nächsten Schritt.
    pub fn steer_agent(&mut self, ticket_key: &str, text: &str) -> bool {
        self.active_run_mut(ticket_key)
            .is_some_and(|r| r.steer(text))
    }

    /// Hält einen laufenden Agenten an.
    pub fn pause_agent(&mut self, ticket_key: &str) -> bool {
        if !self.active_run_mut(ticket_key).is_some_and(AgentRun::pause) {
            return false;
        }
        self.set_agent_note(ticket_key, "Der Agent ist pausiert");
        true
    }

    /// Setzt einen pausierten Agenten fort.
    pub fn resume_agent(&mut self, ticket_key: &str) -> bool {
        if !self
            .active_run_mut(ticket_key)
            .is_some_and(AgentRun::resume)
        {
            return false;
        }
        self.set_agent_note(ticket_key, "Der Agent arbeitet an der Umsetzung");
        true
    }

    /// Lässt jeden laufenden Agenten einen Schritt abschließen oder eine Frage stellen.
    /// Gibt die Schlüssel der Läufe zurück, die dabei fertig geworden sind.
    pub fn advance_agents(&mut self) -> Vec<String> {
        let mut finished = Vec::new();
        let mut asking = Vec::new();
        let mut diagnosed = Vec::new();
        for run in &mut self.runs {
            match run.tick() {
                Tick::Finished => finished.push(run.clone()),
                Tick::Asked => asking.push(run.key()),
                Tick::Diagnosed => diagnosed.push(run.key()),
                Tick::Stepped | Tick::Idle => {}
            }
        }
        for key in &asking {
            self.set_agent_note(key, "Der Agent hat eine Frage an dich");
        }
        for key in &diagnosed {
            self.diagnose_run(key);
        }
        for run in &finished {
            if run.is_followup() {
                self.finish_followup(run);
            } else {
                self.publish(run);
            }
        }
        finished.into_iter().map(|r| r.key()).collect()
    }

    // ---------- Folgeaufträge: Review-Kommentare umsetzen, rote Checks untersuchen ----------

    /// Der Schlüssel des Eintrags, an dem der Pull Request hängt (Ticket oder PR-Eintrag).
    fn item_key_of_pr(&self, number: u32) -> String {
        self.items
            .iter()
            .find(|i| i.pr.as_ref().is_some_and(|p| p.number == number))
            .map_or_else(|| format!("#{number}"), |i| i.key.clone())
    }

    /// Läuft oder wartet schon ein Folgeauftrag auf diesem Pull Request?
    pub fn active_followup(&self, review: u32) -> Option<&AgentRun> {
        self.runs
            .iter()
            .find(|r| r.is_followup() && r.kind.review() == Some(review) && r.is_active())
    }

    /// Alle Folgeaufträge zu einem Pull Request, auch die fertigen.
    pub fn followups(&self, review: u32) -> Vec<&AgentRun> {
        self.runs
            .iter()
            .filter(|r| r.is_followup() && r.kind.review() == Some(review))
            .collect()
    }

    /// Prüft, dass auf dem Pull Request ein Folgeauftrag starten darf.
    fn check_followup(&self, review: u32) -> Result<&Review, AgentError> {
        let r = self.review(review).ok_or(AgentError::UnknownReview)?;
        if r.merged.is_some() {
            return Err(AgentError::Merged);
        }
        if self.active_followup(review).is_some() {
            return Err(AgentError::AlreadyRunning);
        }
        Ok(r)
    }

    fn start_followup(&mut self, mut run: AgentRun, options: AgentOptions) -> String {
        run.start(options, 0);
        let key = run.key();
        let waiting = run.is_awaiting();
        let note = match &run.kind {
            _ if waiting => "Der Agent wartet auf die Freigabe seines Plans",
            RunKind::Diagnose { .. } => "Der Agent untersucht einen roten Check",
            _ => "Der Agent setzt Review-Kommentare um",
        };
        let number = run.pr_number;
        match self.runs.iter_mut().find(|r| r.key() == key) {
            Some(slot) => *slot = run,
            None => self.runs.push(run),
        }
        if let Some(item) = number.and_then(|n| self.pr_item_mut(n)) {
            item.agent_note = Some(note.into());
        }
        key
    }

    /// Beauftragt den Agenten, Kommentare eines Reviews umzusetzen. Es zählen nur offene
    /// Zeilenkommentare dieses Reviews. Gibt den Schlüssel des Laufs zurück (`review-91`).
    ///
    /// Der Lauf pusht auf den Branch des Pull Requests; einen neuen Pull Request gibt es nicht.
    pub fn address_review(
        &mut self,
        review: u32,
        comments: &[String],
        options: AgentOptions,
    ) -> Result<String, AgentError> {
        let r = self.check_followup(review)?;
        let mut files: Vec<String> = Vec::new();
        for id in comments {
            let c = r
                .line_comments
                .iter()
                .find(|c| c.id == *id && c.is_open())
                .ok_or(AgentError::NoComments)?;
            if !files.contains(&c.file) {
                files.push(c.file.clone());
            }
        }
        if comments.is_empty() {
            return Err(AgentError::NoComments);
        }
        let mut ids: Vec<String> = Vec::new();
        for id in comments {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        let (title, branch) = (r.title.clone(), r.branch.clone());
        let mut run = AgentRun::followup(
            RunKind::AddressReview {
                review,
                comments: ids,
            },
            &self.item_key_of_pr(review),
            &title,
            &branch,
        );
        run.files = files;
        Ok(self.start_followup(run, options))
    }

    /// Beauftragt den Agenten, einen roten Check zu untersuchen. Er legt eine Diagnose vor und
    /// ändert nichts, bevor du den Fix übernimmst. Gibt den Schlüssel des Laufs zurück.
    pub fn investigate_check(&mut self, review: u32, check: &str) -> Result<String, AgentError> {
        let r = self.check_followup(review)?;
        let failed = r
            .check_runs
            .iter()
            .any(|c| c.name == check && c.status == Checks::Failed);
        if !failed {
            return Err(AgentError::CheckNotFailed);
        }
        let (title, branch) = (r.title.clone(), r.branch.clone());
        let run = AgentRun::followup(
            RunKind::Diagnose {
                review,
                check: check.to_string(),
            },
            &self.item_key_of_pr(review),
            &title,
            &branch,
        );
        // Eine Untersuchung ändert nichts, bevor du den Fix übernimmst – einen Plan braucht sie nicht.
        let options = AgentOptions {
            plan_first: false,
            ..AgentOptions::default()
        };
        Ok(self.start_followup(run, options))
    }

    /// Die Untersuchung hat das Log gelesen: Die Diagnose wird erstellt und liegt am Lauf.
    fn diagnose_run(&mut self, key: &str) {
        let Some((review, check)) =
            self.runs
                .iter()
                .find(|r| r.key() == key)
                .and_then(|r| match &r.kind {
                    RunKind::Diagnose { review, check } => Some((*review, check.clone())),
                    _ => None,
                })
        else {
            return;
        };
        let diagnosis = self.review(review).and_then(|r| {
            let c = r.check_runs.iter().find(|c| c.name == check)?;
            Some(diagnose(r, c))
        });
        let note = match &diagnosis {
            Some(d) => format!("Diagnose zu „{check}“ liegt vor: {}", d.cause),
            None => format!("Der Check „{check}“ ist nicht mehr da"),
        };
        if let Some(run) = self.runs.iter_mut().find(|r| r.key() == key) {
            run.diagnosis = diagnosis;
        }
        self.set_agent_note(key, &note);
    }

    /// Übernimmt den Fix der Diagnose: Der Agent wendet ihn an, pusht und startet den Check neu.
    pub fn accept_fix(&mut self, key: &str) -> bool {
        if !self.active_run_mut(key).is_some_and(AgentRun::accept_fix) {
            return false;
        }
        self.set_agent_note(key, "Der Agent wendet den Fix an");
        true
    }

    /// Verwirft die Diagnose: Der Lauf endet, der Branch bleibt unberührt.
    pub fn discard_fix(&mut self, key: &str) -> bool {
        if !self.runs.iter().any(|r| r.key() == key && r.is_diagnosed()) {
            return false;
        }
        if let Some(run) = self.active_run_mut(key) {
            run.reset();
        }
        self.clear_agent_note(key);
        self.drop_idle_followups();
        true
    }

    /// Ein Folgeauftrag ist fertig: Der neue Commit steht auf dem Branch des Pull Requests.
    fn finish_followup(&mut self, run: &AgentRun) {
        let Some(number) = run.kind.review() else {
            return;
        };
        match &run.kind {
            RunKind::AddressReview { comments, .. } => {
                let key = run.key();
                let sha = short_sha(&format!("{key}:{}", comments.join(",")));
                let done = self.review_mut(number).map_or(0, |r| {
                    // Änderungsvorschläge landen im Diff, dann sind die Kommentare erledigt.
                    for id in comments {
                        r.apply_comment_suggestion(id);
                    }
                    let done = r.resolve_comments(comments, &key);
                    // Das Review ist eine neue Runde: das alte Urteil gilt nicht mehr.
                    r.verdict = Verdict::Comment;
                    r.sent = false;
                    done
                });
                let what = match done {
                    1 => "1 Review-Kommentar".to_string(),
                    n => format!("{n} Review-Kommentare"),
                };
                if let Some(item) = self.pr_item_mut(number) {
                    if let Some(pr) = item.pr.as_mut() {
                        pr.changes_requested = false;
                    }
                    item.agent_note = Some(format!("Neuer Commit {sha}: {what} umgesetzt"));
                    item.reason = ReasonKind::ReviewRequested;
                    item.reason_text = format!("Review angefragt · Agent hat {what} umgesetzt");
                    item.time = "gerade eben".into();
                    item.read = false;
                    item.comments.push(Comment::new(
                        Person::agent(),
                        "gerade eben",
                        &format!(
                            "Commit {sha} auf {}: {what} umgesetzt. Bitte noch einmal prüfen.",
                            run.branch()
                        ),
                    ));
                }
            }
            RunKind::Diagnose { check, .. } => {
                let fix = run.diagnosis.as_ref().and_then(|d| d.fix.clone());
                let pushed = self
                    .review_mut(number)
                    .zip(fix.as_ref())
                    .is_some_and(|(r, fix)| {
                        let applied = r.apply_fix(fix);
                        if applied {
                            // Der Fix behebt den Fehler; ein wackliger Check braucht nur den Neustart.
                            r.set_rerun(check, Checks::Passed);
                        }
                        applied
                    });
                self.restart_check(number, check);
                let sha = short_sha(&format!("{}:{check}", run.key()));
                let note = if pushed {
                    format!("Fix {sha} gepusht, „{check}“ läuft erneut")
                } else {
                    format!("„{check}“ neu gestartet")
                };
                if let Some(item) = self.pr_item_mut(number) {
                    item.agent_note = Some(note.clone());
                    if pushed {
                        item.comments.push(Comment::new(
                            Person::agent(),
                            "gerade eben",
                            &format!("Commit {sha} auf {}: {note}.", run.branch()),
                        ));
                    }
                }
            }
            RunKind::Implement => {}
        }
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

        let mut seen = None;
        if let Some(item) = self.items.iter_mut().find(|i| i.key == run.ticket_key) {
            item.branch = Some(run.branch());
            item.pr = Some(PrRef {
                number,
                draft: run.options.draft_pr,
                checks,
                changes_requested: false,
                merged: false,
                review_requested: true,
            });
            // Der Pull Request wartet als Review – derselbe Weg wie bei jedem verknüpften Ticket.
            item.status = item
                .pr
                .as_ref()
                .map_or(TicketStatus::Review, PrRef::target_status);
            seen = Some((item.key.clone(), item.status));
            item.agent_note = Some(format!("Vom Agenten umgesetzt · {tests}"));
            item.reason = ReasonKind::AgentFinished;
            item.reason_text = format!("Agent fertig · Pull Request #{number} erstellt");
            item.time = "gerade eben".into();
            item.read = false;
        }
        if let Some((key, status)) = seen {
            self.mark_status_seen(&key, status);
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
            check_runs: {
                let mut runs = demo::passing_checks();
                // Ohne Tests läuft der Test-Check noch; der Takt der Checks schließt ihn ab.
                if checks == Checks::Running
                    && let Some(tests) = runs.last_mut()
                {
                    tests.status = Checks::Running;
                    tests.remaining = CHECK_STEPS;
                    tests.log = "$ cargo test\nwird ausgeführt …\n".into();
                }
                runs
            },
            merged: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::STEPS;
    use crate::attention::Rule;

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

    /// Lässt alle Agenten zu Ende laufen und beantwortet dabei jede Rückfrage.
    fn finish_agents(w: &mut Workspace) -> Vec<String> {
        let mut finished = Vec::new();
        for _ in 0..=STEPS {
            let asking: Vec<String> = w
                .runs
                .iter()
                .filter(|r| r.is_asking())
                .map(|r| r.ticket_key.clone())
                .collect();
            for key in asking {
                assert!(w.answer_agent(&key, "Entfernen"));
            }
            finished.extend(w.advance_agents());
        }
        finished
    }

    #[test]
    fn demo_data_matches_the_design() {
        let w = ws();
        assert_eq!(w.items.len(), 5);
        assert_eq!(w.unread(Filter::All, NOW), 4, "pr85 gehört nicht mir");
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
        assert_eq!(w.unread(Filter::Mine, NOW), 3);
        w.mark_done("bau142");
        assert_eq!(w.unread(Filter::All, NOW), 3);
        assert_eq!(w.unread(Filter::Mine, NOW), 2);
        assert_eq!(w.unread(Filter::Team, NOW), 1);
    }

    /// 11.11.2026, 12:00 UTC.
    const NOW: u64 = 1_794_398_400_000;
    const HOUR: u64 = 3_600_000;

    fn ids(items: Vec<&Item>) -> Vec<&str> {
        items.into_iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn the_inbox_follows_the_rules() {
        let mut w = ws();
        // pr85 (Checks rot, aber von Mira) trifft keine Regel und fehlt.
        assert_eq!(
            ids(w.attention(Filter::All, NOW)),
            ["bau142", "pr87", "bau138", "bau150"]
        );
        assert_eq!(ids(w.attention(Filter::Team, NOW)), ["pr87"]);
        // Regel aus: die Treffer dahinter verschwinden – bau150 bleibt wegen „Zugewiesen“.
        w.set_rule(Rule::ReviewRequested, false);
        assert_eq!(ids(w.attention(Filter::Team, NOW)), Vec::<&str>::new());
        w.set_rule(Rule::Overdue, false);
        w.set_rule(Rule::Assigned, false);
        assert!(!ids(w.attention(Filter::All, NOW)).contains(&"bau150"));
        // Der Grund des Eintrags ist der erste Treffer.
        assert_eq!(
            w.hits(w.item("bau142").unwrap(), NOW)[0].rule,
            Rule::Mention
        );
        // Antworten erledigt die Erwähnung.
        w.reply("bau142", "Mache ich.");
        w.set_rule(Rule::Assigned, true);
        assert_eq!(
            w.hits(w.item("bau142").unwrap(), NOW)[0].rule,
            Rule::Assigned
        );
    }

    #[test]
    fn counts_follow_the_rules_and_snoozing() {
        let mut w = ws();
        assert_eq!(w.unread(Filter::Mine, NOW), 3);
        w.set_rule(Rule::Mention, false);
        w.set_rule(Rule::Assigned, false);
        // bau142 hat nun keinen Grund mehr; bau138 (blockiert) und bau150 (überfällig) bleiben.
        assert_eq!(w.unread(Filter::Mine, NOW), 2);
        assert!(w.snooze("bau138", Some(NOW + HOUR)));
        assert_eq!(w.unread(Filter::Mine, NOW), 1);
        assert_eq!(w.unread(Filter::Snoozed, NOW), 1);
        // Nach Ablauf zählt er wieder – auch ohne dass jemand `wake` aufruft.
        assert_eq!(w.unread(Filter::Mine, NOW + HOUR), 2);
        assert_eq!(w.unread(Filter::Snoozed, NOW + HOUR), 0);
    }

    #[test]
    fn snoozed_items_leave_the_inbox_and_show_in_their_filter() {
        let mut w = ws();
        assert!(w.snooze("pr87", Some(NOW + HOUR)));
        assert!(!w.snooze("gibt-es-nicht", None));
        assert!(!ids(w.attention(Filter::All, NOW)).contains(&"pr87"));
        assert_eq!(ids(w.attention(Filter::Snoozed, NOW)), ["pr87"]);
        assert_eq!(
            ids(w.search(Filter::Snoozed, SourceFilter::GitHub, "slider", NOW)),
            ["pr87"]
        );
        assert!(
            w.search(Filter::Snoozed, SourceFilter::Jira, "", NOW)
                .is_empty()
        );
        // Suche und Quelle arbeiten auf den Regeln, nicht auf allen Einträgen.
        assert!(
            w.search(Filter::All, SourceFilter::All, "pr85", NOW)
                .is_empty()
        );
        // Erneut zurückstellen ersetzt die alte Zurückstellung.
        w.snooze("pr87", None);
        assert_eq!(w.snoozed.len(), 1);
        assert!(w.unsnooze("pr87"));
        assert!(!w.unsnooze("pr87"));
        assert!(ids(w.attention(Filter::All, NOW)).contains(&"pr87"));
    }

    #[test]
    fn wake_removes_only_what_expired() {
        let mut w = ws();
        w.snooze("pr87", Some(NOW + HOUR));
        w.snooze("bau150", Some(NOW + 5 * HOUR));
        w.snooze("bau138", None);
        assert!(w.wake(NOW).is_empty());
        assert_eq!(w.wake(NOW + 2 * HOUR), ["pr87"]);
        assert_eq!(w.snoozed.len(), 2);
        // „Bis sich etwas ändert“: ein Kommentar holt den Eintrag zurück.
        w.reply("bau138", "Hallo");
        assert!(ids(w.attention(Filter::All, NOW)).contains(&"bau138"));
        assert_eq!(w.expired_snoozes(NOW), ["bau138"]);
        assert_eq!(w.wake(NOW), ["bau138"]);
        assert_eq!(w.snoozed.len(), 1);
    }

    #[test]
    fn a_status_change_wakes_a_change_snooze() {
        let mut w = ws();
        w.snooze("bau150", None);
        assert!(w.is_snoozed("bau150", NOW + 1000 * HOUR));
        w.set_status("bau150", TicketStatus::InProgress);
        assert!(!w.is_snoozed("bau150", NOW));
    }

    #[test]
    fn answering_a_review_request_ends_the_attention() {
        let mut w = ws();
        for (s, d) in [
            ("a", Decision::Approved),
            ("b", Decision::Approved),
            ("c", Decision::Approved),
        ] {
            w.decide(87, s, Some(d));
        }
        w.send_review(87, "").unwrap();
        assert!(!ids(w.attention(Filter::All, NOW)).contains(&"pr87"));
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
        assert_eq!(w.add_line_comment(1, &file, 0, "x", None), None);
        let id = w
            .add_line_comment(87, &file, 0, "Warum hier?", None)
            .unwrap();
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
        assert!(w.edit_line_comment(87, &id, "Anders", None));
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
                checks: Checks::Running,
                changes_requested: false,
                merged: false,
                review_requested: true,
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

    #[test]
    fn an_agent_pull_request_without_tests_gets_green_when_its_check_finishes() {
        let mut w = ws();
        let options = AgentOptions {
            run_tests: false,
            ..direct()
        };
        w.start_agent("BAU-150", options).unwrap();
        finish_agents(&mut w);
        assert_eq!(w.review(92).unwrap().checks, Checks::Running);
        assert_eq!(w.running_checks(), 1);
        assert_eq!(w.merge_blockers(92)[0], MergeBlocker::Draft);
        for _ in 0..CHECK_STEPS {
            w.advance_checks();
        }
        assert_eq!(w.review(92).unwrap().checks, Checks::Passed);
        assert_eq!(
            w.item("bau150").unwrap().pr.as_ref().unwrap().checks,
            Checks::Passed
        );
    }
}
