//! People and their comments.

use super::*;

/// How a plain name field (reporter, co-assignees, `@mention` in a comment text) marks me. It is
/// a stored marker, not a text for people: the interface shows [`Person::me`] through the catalog.
pub const ME_MARK: &str = "Du";

/// The initials of nobody (an unassigned ticket). A sign, not a word.
const NOBODY_INITIALS: &str = "–";

/// A person as the service names them, or one of two roles the interface words itself: me and
/// nobody. For a role the name is not stored; [`Person::name_in`] and [`Person::initials_in`]
/// give the words in the language of the interface.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(from = "PersonWire")]
pub struct Person {
    pub name: String,
    pub initials: String,
    /// The signed-in user.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub me: bool,
    /// Nobody: the owner of a ticket that is not assigned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nobody: bool,
}

/// What is stored of a person. Older data holds me as the name `Du` and nobody with the
/// initials `–` and a German name; both read as the roles.
#[derive(Deserialize)]
struct PersonWire {
    #[serde(default)]
    name: String,
    #[serde(default)]
    initials: String,
    #[serde(default)]
    me: bool,
    #[serde(default)]
    nobody: bool,
}

impl From<PersonWire> for Person {
    fn from(w: PersonWire) -> Self {
        if w.me || (!w.nobody && w.name == ME_MARK) {
            Person::me()
        } else if w.nobody || w.initials == NOBODY_INITIALS {
            Person::nobody()
        } else {
            Person::new(&w.name, &w.initials)
        }
    }
}

impl Person {
    pub fn new(name: &str, initials: &str) -> Self {
        Self {
            name: name.into(),
            initials: initials.into(),
            me: false,
            nobody: false,
        }
    }

    /// The signed-in user.
    pub fn me() -> Self {
        Self {
            me: true,
            ..Self::new("", "")
        }
    }

    /// Nobody: the owner of a ticket that is not assigned.
    pub fn nobody() -> Self {
        Self {
            nobody: true,
            ..Self::new("", NOBODY_INITIALS)
        }
    }

    pub fn agent() -> Self {
        Self::new("Agent", "AG")
    }

    pub fn is_agent(&self) -> bool {
        *self == Self::agent()
    }

    /// The name as a message: the role in the catalog, a person as the service names them.
    pub fn name_msg(&self) -> cockpit_i18n::Msg {
        if self.me {
            cockpit_i18n::msg!(PersonMe)
        } else if self.nobody {
            cockpit_i18n::msg!(PersonNobody)
        } else {
            cockpit_i18n::Msg::raw(self.name.clone())
        }
    }

    /// The name in `lang`.
    pub fn name_in(&self, lang: cockpit_i18n::Lang) -> String {
        self.name_msg().render(lang)
    }

    /// The initials for an avatar in `lang`.
    pub fn initials_in(&self, lang: cockpit_i18n::Lang) -> String {
        if self.me {
            cockpit_i18n::plain(lang, cockpit_i18n::Key::PersonMeInitials)
        } else {
            self.initials.clone()
        }
    }

    /// The words after an `@` that mention this person, in lower case ([`ME_MARK`] for me).
    pub fn handles(&self) -> Vec<String> {
        if self.me {
            vec![ME_MARK.to_lowercase()]
        } else if self.nobody {
            Vec::new()
        } else {
            [&self.name, &self.initials]
                .into_iter()
                .filter(|h| !h.is_empty())
                .map(|h| h.to_lowercase())
                .collect()
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub author: Person,
    /// The time of older data as plain German text ("vor 5 Minuten"); new comments carry
    /// [`Comment::created_ms`] and leave this empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub time: String,
    /// When the comment was written (milliseconds since 1970), `None` = unknown. The interface
    /// shows it as "5 minutes ago".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_ms: Option<u64>,
    #[serde(default)]
    pub text: String,
    /// The text as code and parameters (comments the agent writes); `None` = only [`Comment::text`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_msg: Option<cockpit_i18n::Msg>,
    /// The comment as formatted Jira text (ADF); `None` = only [`Comment::text`] exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rich: Option<crate::adf::Doc>,
    /// Written by me.
    #[serde(default)]
    pub own: bool,
}

impl Comment {
    /// A comment with the time as plain text (older data, fixtures).
    pub fn new(author: Person, time: &str, text: &str) -> Self {
        Self {
            author,
            time: time.into(),
            created_ms: None,
            text: text.into(),
            text_msg: None,
            rich: None,
            own: false,
        }
    }

    /// A comment written at `created_ms`.
    pub fn at(author: Person, created_ms: u64, text: &str) -> Self {
        Self {
            created_ms: Some(created_ms),
            ..Self::new(author, "", text)
        }
    }
}
