//! People and their comments.

use super::*;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Person {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub initials: String,
}

impl Person {
    pub fn new(name: &str, initials: &str) -> Self {
        Self {
            name: name.into(),
            initials: initials.into(),
        }
    }

    /// The signed-in user.
    pub fn me() -> Self {
        Self::new("Du", "AS")
    }

    pub fn agent() -> Self {
        Self::new("Agent", "AG")
    }

    pub fn is_agent(&self) -> bool {
        *self == Self::agent()
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
