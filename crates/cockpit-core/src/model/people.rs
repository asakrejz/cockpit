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
    #[serde(default)]
    pub time: String,
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
    /// A comment as code and parameters; `text` keeps the German rendering next to it.
    pub fn coded(author: Person, time: &str, msg: cockpit_i18n::Msg) -> Self {
        Self {
            text_msg: Some(msg.clone()),
            ..Self::new(author, time, &msg.de())
        }
    }

    pub fn new(author: Person, time: &str, text: &str) -> Self {
        Self {
            author,
            time: time.into(),
            text: text.into(),
            text_msg: None,
            rich: None,
            own: false,
        }
    }
}
