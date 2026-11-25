//! Who did something, what it concerned and the log entry.

use super::*;

/// Who triggered the action.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    #[serde(alias = "Du")]
    User,
    Agent,
    System,
}

impl Actor {
    pub const ALL: [Actor; 3] = [Actor::User, Actor::Agent, Actor::System];

    pub fn label(self) -> &'static str {
        match self {
            Actor::User => "Du",
            Actor::Agent => "Agent",
            Actor::System => "System",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Actor::User => "du",
            Actor::Agent => "agent",
            Actor::System => "system",
        }
    }

    pub fn from_id(id: &str) -> Option<Actor> {
        Actor::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// What a log entry refers to.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// An inbox entry (internal ID).
    Item(String),
    /// A review (the pull request; old logs hold the plain number).
    Review(PrKey),
    /// The run of an agent (ticket key).
    Run(String),
    /// The settings (notifications).
    Settings,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Running number, rises with every entry.
    pub id: u64,
    /// Point in time in milliseconds since 1970.
    pub at_ms: u64,
    pub actor: Actor,
    #[serde(default)]
    pub target: Option<Target>,
    /// The sentence of an old entry as plain German text; new entries carry `msg` and leave
    /// this empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// The sentence as code and parameters. Shown in the language of the interface; without it,
    /// or with a code this build no longer knows, `text` is shown as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<cockpit_i18n::Msg>,
}

impl LogEntry {
    /// The sentence in `lang`.
    pub fn text_in(&self, lang: cockpit_i18n::Lang) -> String {
        self.msg
            .as_ref()
            .map_or_else(|| self.text.clone(), |m| m.render_or(lang, &self.text))
    }
}

/// Result of a successful action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// Number of new log entries.
    pub logged: usize,
}
