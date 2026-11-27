//! The people of the overview per source: which source the overview shows, and which Jira
//! person is the same human as which GitHub login.
//!
//! Jira names and GitHub logins are different identities. They are never merged by guess; only a
//! link the user sets ([`PersonLink`]) joins them. You ([`Person::me`]) need no link: your Jira
//! account and your GitHub login are both known, so both already map to the same person.

use serde::{Deserialize, Serialize};

use crate::model::{Person, Source};
use crate::workspace::Workspace;

/// Which source the overview shows. GitHub covers pull requests and GitHub Projects tickets.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PeopleSource {
    #[default]
    All,
    Jira,
    GitHub,
}

impl PeopleSource {
    /// All choices, in display order.
    pub const ALL: [PeopleSource; 3] =
        [PeopleSource::All, PeopleSource::Jira, PeopleSource::GitHub];

    pub fn id(self) -> &'static str {
        match self {
            PeopleSource::All => "all",
            PeopleSource::Jira => "jira",
            PeopleSource::GitHub => "github",
        }
    }

    /// Does the filter let entries of this source through?
    pub fn allows(self, source: Source) -> bool {
        match self {
            PeopleSource::All => true,
            PeopleSource::Jira => source == Source::Jira,
            PeopleSource::GitHub => source != Source::Jira,
        }
    }
}

/// A Jira person and a GitHub login the user says are the same human.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PersonLink {
    /// Display name of the Jira person.
    #[serde(default)]
    pub jira: String,
    /// Login of the GitHub user.
    #[serde(default)]
    pub github: String,
}

impl Workspace {
    /// Choose the source of the overview. `false` if nothing changed.
    pub fn set_people_source(&mut self, source: PeopleSource) -> bool {
        if self.people_source == source {
            return false;
        }
        self.people_source = source;
        true
    }

    /// Link a Jira person to a GitHub login. An existing link of either side is replaced, so every
    /// name has at most one link. `false` if nothing changed or the input is not a link
    /// (empty, you, the agent).
    pub fn link_people(&mut self, jira: &str, github: &str) -> bool {
        let (jira, github) = (jira.trim(), github.trim());
        if !linkable(jira) || !linkable(github) {
            return false;
        }
        let link = PersonLink {
            jira: jira.into(),
            github: github.into(),
        };
        if self.person_links.contains(&link) {
            return false;
        }
        self.person_links
            .retain(|l| l.jira != link.jira && l.github != link.github);
        self.person_links.push(link);
        true
    }

    /// Remove the link of a Jira person. `false` if there was none.
    pub fn unlink_people(&mut self, jira: &str) -> bool {
        let before = self.person_links.len();
        self.person_links.retain(|l| l.jira != jira);
        self.person_links.len() != before
    }

    /// The link of a Jira person.
    pub fn link_of_jira(&self, jira: &str) -> Option<&PersonLink> {
        self.person_links.iter().find(|l| l.jira == jira)
    }

    /// The link of a GitHub login.
    pub fn link_of_github(&self, github: &str) -> Option<&PersonLink> {
        self.person_links.iter().find(|l| l.github == github)
    }
}

impl Workspace {
    /// The people a member of `side` can be linked to: those of the other source (their entries
    /// or, for GitHub, the authors of reviews) who are not linked yet, not you, not the agent.
    /// Independent of the source filter.
    pub fn link_candidates(&self, side: crate::team::Side) -> Vec<Person> {
        use crate::team::Side;
        let other = match side {
            Side::Jira => Side::GitHub,
            Side::GitHub => Side::Jira,
        };
        let mut out: Vec<Person> = Vec::new();
        let mut add = |p: &Person| {
            let taken = match other {
                Side::Jira => self.link_of_jira(&p.name).is_some(),
                Side::GitHub => self.link_of_github(&p.name).is_some(),
            };
            if linkable(&p.name) && !taken && !out.iter().any(|o| o.name == p.name) {
                out.push(p.clone());
            }
        };
        for item in &self.items {
            if Side::of(item.source) == other {
                add(&item.owner);
            }
        }
        if other == Side::GitHub {
            for review in &self.reviews {
                add(&review.author);
            }
        }
        out
    }
}

fn linkable(name: &str) -> bool {
    !name.is_empty() && name != crate::model::ME_MARK && name != Person::agent().name
}
