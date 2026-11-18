//! `POST /api/translate`: translating a text from Jira or GitHub on request.

use serde::{Deserialize, Serialize};

/// What to translate. The text is sent to the model of the agent: the interface warns before the
/// first time.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TranslateRequest {
    pub text: String,
    /// Language to translate into.
    pub to: cockpit_i18n::Lang,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TranslateResponse {
    pub text: String,
}
