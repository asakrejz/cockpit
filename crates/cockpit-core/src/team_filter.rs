//! „Mein Team“: which tickets of the Jira „Team“ field the lists show.
//!
//! The choice ([`TeamChoice`]) is a setting of the workspace. Everything here is a pure rule over
//! the workspace; the overview ([`crate::team`]) and the inbox ([`Workspace::inbox`]) ask
//! [`Workspace::team_visible`].
//!
//! * No team chosen: nothing is filtered.
//! * A ticket is shown if its team is chosen; a ticket without a team only if
//!   [`TeamChoice::include_no_team`] is on.
//! * What is mine ([`Scope::Mine`]) is always shown.
//! * A pull request is not filtered, unless it is linked to tickets (ticket key in branch or
//!   title, or the ticket carries the pull request) and none of them is shown: then it is hidden too.

use serde::{Deserialize, Serialize};

use crate::link::ticket_keys;
use crate::model::{Item, Kind, TeamRef};
use crate::workspace::Workspace;

/// The teams the user belongs to.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct TeamChoice {
    /// The chosen teams (the name is kept so that a team without tickets can still be shown).
    #[serde(default)]
    pub teams: Vec<TeamRef>,
    /// Also show tickets that have no team.
    #[serde(default)]
    pub include_no_team: bool,
}

impl TeamChoice {
    /// Is a team chosen at all? Without one nothing is filtered.
    pub fn is_active(&self) -> bool {
        !self.teams.is_empty()
    }

    /// Nothing chosen and nothing set (the start value, left out when saving).
    pub fn is_default(&self) -> bool {
        self.teams.is_empty() && !self.include_no_team
    }

    /// The choice without empty IDs and duplicates.
    pub fn cleaned(mut self) -> Self {
        let mut seen: Vec<String> = Vec::new();
        self.teams.retain(|t| {
            let id = t.id.trim();
            let fresh = !id.is_empty() && !seen.iter().any(|s| s == id);
            if fresh {
                seen.push(id.to_string());
            }
            fresh
        });
        self
    }

    /// Is a ticket with this team shown?
    pub fn allows(&self, team: Option<&TeamRef>) -> bool {
        if !self.is_active() {
            return true;
        }
        match team {
            Some(t) => self.teams.iter().any(|c| c.id == t.id),
            None => self.include_no_team,
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.teams.iter().any(|t| t.id == id)
    }
}

impl Workspace {
    /// The teams of the tickets (and the chosen ones), by name, without duplicates.
    pub fn teams(&self) -> Vec<TeamRef> {
        let mut out: Vec<TeamRef> = Vec::new();
        for t in self
            .items
            .iter()
            .filter_map(|i| i.team.as_ref())
            .chain(self.team_choice.teams.iter())
        {
            if !t.id.is_empty() && !out.iter().any(|o| o.id == t.id) {
                out.push(t.clone());
            }
        }
        out.sort_by_key(|t| (t.name.to_lowercase(), t.id.clone()));
        out
    }

    /// Does any ticket carry a team? Without, the team field is not there (or not set up) and
    /// the interface has nothing to choose from.
    pub fn has_team_data(&self) -> bool {
        self.items.iter().any(|i| i.team.is_some())
    }

    /// Replaces the choice. `false` if nothing changes.
    pub fn set_team_choice(&mut self, choice: TeamChoice) -> bool {
        let choice = choice.cleaned();
        if self.team_choice == choice {
            return false;
        }
        self.team_choice = choice;
        true
    }

    /// Does the chosen team (or the lack of one) let this entry through? See the module.
    pub fn team_visible(&self, item: &Item) -> bool {
        let choice = &self.team_choice;
        if !choice.is_active() || item.is_mine() {
            return true;
        }
        match item.kind {
            Kind::Ticket => choice.allows(item.team.as_ref()),
            Kind::PullRequest => {
                let linked = self.linked_tickets(item);
                linked.is_empty() || linked.iter().any(|t| self.team_visible(t))
            }
        }
    }

    /// The tickets a pull request belongs to.
    fn linked_tickets(&self, pr: &Item) -> Vec<&Item> {
        let own = pr.pr.as_ref().map(|p| p.key());
        let mut keys: Vec<String> = Vec::new();
        let repo = pr.pr.as_ref().map_or("", |p| p.repo.as_str());
        for text in [pr.branch.as_deref().unwrap_or_default(), pr.title.as_str()] {
            keys.extend(ticket_keys(text));
            // GitHub issues: `#12`, `acme/app#12`, `Fixes #12`.
            keys.extend(crate::project::issue_refs(text, repo).iter().map(|r| r.key()));
        }
        self.items
            .iter()
            .filter(|t| t.kind == Kind::Ticket)
            .filter(|t| {
                keys.iter().any(|k| k.eq_ignore_ascii_case(&t.key))
                    || own
                        .as_ref()
                        .zip(t.pr.as_ref())
                        .is_some_and(|(a, b)| a.same(&b.key()))
                    || own.as_ref().is_some_and(|a| {
                        t.project
                            .as_ref()
                            .is_some_and(|p| p.linked_prs.iter().any(|k| k.same(a)))
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
