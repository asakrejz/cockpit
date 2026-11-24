//! Pull request identity: key (repo + number) and reference.

use super::*;

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

impl Serialize for PrKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.repo.is_empty() {
            s.serialize_u32(self.number)
        } else {
            s.collect_str(self)
        }
    }
}

/// A repo name `owner/name`, spelled the way GitHub spells it, with exactly one slash:
///
/// * the owner has 1 to 39 letters, digits and single hyphens, none at either end;
/// * the name has 1 to 100 letters, digits, `.`, `_` and `-`, and is never `.` or `..`.
///
/// This is the one rule for every repo name Cockpit reads, stores or hands to `git` and the
/// container. The agent runner repeats it in `apps/agent/runner/src/job.ts` as a second line of
/// defence; both are tested against `tests/fixtures/repo_name_cases.json`, so a change to the
/// rule has to change that file and then both sides have to agree again.
///
/// ```
/// use cockpit_core::repo_name_valid;
/// assert!(repo_name_valid("acme/app.js"));
/// assert!(!repo_name_valid("acme/.."));
/// assert!(!repo_name_valid("acme/app/extra"));
/// ```
pub fn repo_name_valid(repo: &str) -> bool {
    let Some((owner, name)) = repo.split_once('/') else {
        return false;
    };
    let owner_ok = (1..=39).contains(&owner.len())
        && !owner.starts_with('-')
        && !owner.ends_with('-')
        && !owner.contains("--")
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let name_ok = (1..=100).contains(&name.len())
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    owner_ok && name_ok
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
    /// GitHub login of the author (empty = not known, old data).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    /// The pull request concerns me: I am the author, a requested reviewer or an assignee.
    /// Old data has `false` here; the entry's `scope` stays the authority for old entries.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mine: bool,
    /// A review from me is requested (as a person, not through a team).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub requested_me: bool,
    /// I am an assignee.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub assigned_me: bool,
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

    /// The page of the pull request at github.com; `None` if the repo is not known.
    pub fn github_url(&self) -> Option<String> {
        (repo_name_valid(&self.repo) && self.number > 0)
            .then(|| format!("https://github.com/{}/pull/{}", self.repo, self.number))
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

impl From<&PrKey> for PrKey {
    fn from(key: &PrKey) -> Self {
        key.clone()
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
