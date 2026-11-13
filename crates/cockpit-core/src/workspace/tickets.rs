use super::*;

impl Workspace {
    // ---------- Tickets and pull requests ----------

    pub fn mark_done(&mut self, id: &str) {
        if let Some(item) = self.item_mut(id) {
            item.read = true;
        }
    }

    /// Changes the status of a ticket. Pull requests have none.
    pub fn set_status(&mut self, id: &str, status: TicketStatus) -> bool {
        match self.item_mut(id) {
            Some(item) if item.is_ticket() => {
                item.status = status;
                true
            }
            _ => false,
        }
    }

    /// Creates a branch for an entry without one and returns the name.
    pub fn create_branch(&mut self, id: &str) -> Option<String> {
        let item = self.item_mut(id).filter(|i| i.branch.is_none())?;
        let name = format!("feature/{}-{}", item.key, slug(&item.title, 24));
        item.branch = Some(name.clone());
        Some(name)
    }

    /// Replies in the ticket or pull request. Empty text is ignored.
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

    // ---------- Edit tickets ----------

    /// Changes title and description of a ticket. The title must not be empty.
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

    /// People a ticket can be assigned to: first you, then all others who appear in the
    /// inbox, as the author of a pull request or in the comments. The agent gets tickets through
    /// its run, not through the assignment.
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

    /// Assigns a ticket to someone. Who owns it also decides whether it appears under „Ich“ or „Team“. A ticket
    /// that newly lands with you counts as unread again and triggers the rule
    /// „Zugewiesen“; if it goes to someone else, it ends.
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

    // ---------- Link ticket and pull request ----------

    /// The ticket that belongs to this pull request (via the review).
    pub fn ticket_of_pr(&self, number: impl Into<PrKey>) -> Option<&Item> {
        let key = self.review(number)?.ticket_key.as_deref()?;
        self.item_by_key(key).filter(|i| i.is_ticket())
    }

    /// The copy of the pull request for a ticket: the entry from the inbox, otherwise from the review.
    pub(super) fn pr_ref_of(&self, number: impl Into<PrKey>) -> Option<PrRef> {
        let key = number.into();
        let from_item = self
            .items
            .iter()
            .filter(|i| i.kind == Kind::PullRequest)
            .find_map(|i| i.pr.clone().filter(|p| key.matches(&p.repo, p.number)));
        from_item.or_else(|| {
            self.review(&key).map(|r| PrRef {
                number: r.number,
                repo: r.repo.clone(),
                draft: false,
                checks: r.checks,
                changes_requested: false,
                merged: r.merged.is_some(),
                review_requested: false,
            })
        })
    }

    /// Links a ticket with a pull request. The ticket gets the pull request (and,
    /// if it has none yet, the branch), the review gets the ticket.
    pub fn link_pr(&mut self, ticket_id: &str, number: impl Into<PrKey>) -> Result<(), LinkError> {
        let number = number.into();
        let key = self
            .item(ticket_id)
            .map(|i| i.key.clone())
            .unwrap_or_default();
        let review = self
            .review(&number)
            .ok_or_else(|| LinkError::UnknownPr(number.clone()))?;
        let branch = review.branch.clone();
        let number = review.key();
        if let Some(other) = self.ticket_of_pr(&number).map(|t| t.key.clone()) {
            return Err(if other == key {
                LinkError::Same
            } else {
                LinkError::PrHasTicket(other)
            });
        }
        let current = self
            .item(ticket_id)
            .and_then(|i| i.pr.as_ref().map(PrRef::key));
        if let Some(n) = current {
            // If the ticket already points to this pull request, only the other side is missing.
            if !n.same(&number)
                && self
                    .review(&n)
                    .is_some_and(|r| r.ticket_key.as_deref() == Some(&key))
            {
                return Err(LinkError::TicketHasPr(n));
            }
        }
        let pr = self.pr_ref_of(&number);
        if let Some(item) = self.item_mut(ticket_id) {
            item.pr = pr;
            if item.branch.is_none() && !branch.is_empty() {
                item.branch = Some(branch);
            }
        }
        if let Some(review) = self.review_mut(&number) {
            review.ticket_key = Some(key.clone());
        }
        self.unlinked
            .retain(|(n, k)| !(n.same(&number) && *k == key));
        Ok(())
    }

    /// Removes the link of a ticket. Returns the pull request.
    /// The automation does not link the same pair again.
    pub fn unlink_pr(&mut self, ticket_id: &str) -> Option<PrKey> {
        let item = self.item(ticket_id).filter(|i| i.is_ticket())?;
        let (key, number) = (item.key.clone(), item.pr.as_ref()?.key());
        let review_branch = self.review(&number).map(|r| r.branch.clone());
        if let Some(review) = self
            .review_mut(&number)
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
        if !self
            .unlinked
            .iter()
            .any(|(n, k)| n.same(&number) && *k == key)
        {
            self.unlinked.push((number.clone(), key));
        }
        Some(number)
    }

    /// Which pull requests the automation would link now: `(ticket ID, pull request)`.
    /// A pull request qualifies as long as no existing ticket belongs to it and
    /// branch or title name the key of a ticket without a pull request. Pairs
    /// resolved by hand stay resolved.
    pub fn auto_links(&self) -> Vec<(String, PrKey)> {
        let mut taken: Vec<&str> = Vec::new();
        let mut links = Vec::new();
        for review in self
            .reviews
            .iter()
            .filter(|r| self.ticket_of_pr(r.key()).is_none())
        {
            let free = self.items.iter().filter(|i| {
                i.is_ticket()
                    && i.pr.is_none()
                    && !taken.contains(&i.key.as_str())
                    && !self
                        .unlinked
                        .iter()
                        .any(|(n, k)| n.same(&review.key()) && *k == i.key)
            });
            let known = free.clone().map(|i| i.key.as_str());
            if let Some(key) = crate::link::find_ticket(&[&review.branch, &review.title], known)
                && let Some(ticket) = free.clone().find(|i| i.key == key)
            {
                taken.push(&ticket.key);
                links.push((ticket.id.clone(), review.key()));
            }
        }
        links
    }

    /// Links what `auto_links` finds. Returns `(ticket ID, key, pull request)`.
    pub fn auto_link(&mut self) -> Vec<(String, String, PrKey)> {
        let mut done = Vec::new();
        for (id, number) in self.auto_links() {
            if self.link_pr(&id, &number).is_ok() {
                let key = self.item(&id).map(|i| i.key.clone()).unwrap_or_default();
                done.push((id, key, number));
            }
        }
        done
    }

    // ---------- Status follows the pull request ----------

    /// Linked tickets with the status their pull request suggests: `(index, pull request, target)`.
    pub(super) fn linked_targets(&self) -> Vec<(usize, PrKey, TicketStatus)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.is_ticket())
            .filter_map(|(at, i)| {
                let pr = i.pr.as_ref()?;
                let is_linked = self
                    .review(pr.key())
                    .is_some_and(|r| r.ticket_key.as_deref() == Some(i.key.as_str()));
                is_linked.then(|| (at, pr.key(), pr.target_status()))
            })
            .collect()
    }

    /// Has a pull request moved since the last bring-in-line? (Without changing anything.)
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

    /// Notes that the status matches the current state of the pull request, without changing it.
    pub(super) fn mark_status_seen(&mut self, key: &str, target: TicketStatus) {
        match self.status_seen.iter_mut().find(|(k, _)| k == key) {
            Some((_, seen)) => *seen = target,
            None => self.status_seen.push((key.to_string(), target)),
        }
    }

    /// Brings the status of linked tickets in line: pull request merged → Done, review
    /// requested → Review, otherwise → In progress. Only if the pull request's state has changed since
    /// the last time; a status you set by hand afterwards stays
    /// until the pull request moves again. With the automation switched off it does nothing.
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

    /// Switches „status follows the pull request“ on or off. `false` if nothing changes.
    pub fn set_status_follows_pr(&mut self, enabled: bool) -> bool {
        if self.pr_status_off != enabled {
            return false;
        }
        self.pr_status_off = !enabled;
        true
    }
}
