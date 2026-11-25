//! Wire shapes of the webhook endpoints.

use super::*;

/// Switch and public address for webhooks. `None` leaves a value unchanged; an empty address
/// removes it. The address is only shown to the user (for the two target URLs), never called.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhooksInput {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub public_url: Option<String>,
}

/// What the assistant shows about one webhook source. Never the secret.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhookSourceInfo {
    /// Path of the route on this server, e.g. `/api/webhooks/github`.
    #[serde(default)]
    pub path: String,
    /// A secret is in the `.env` file.
    #[serde(default)]
    pub secret_set: bool,
    /// When a correctly signed request last arrived (milliseconds since 1970); memory only.
    #[serde(default)]
    pub last_received_ms: Option<u64>,
    /// Its event type (`ping`, `pull_request`, `jira:issue_updated`, ...), nothing else of it.
    #[serde(default)]
    pub last_event: Option<String>,
}

/// State of the optional webhooks (`GET /api/setup`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhooksInfo {
    #[serde(default)]
    pub enabled: bool,
    /// `[server] public_url`: the address of the tunnel, for display only.
    #[serde(default)]
    pub public_url: Option<String>,
    /// Poll interval of a source while its webhooks arrive regularly.
    #[serde(default)]
    pub fallback_interval_s: u64,
    #[serde(default)]
    pub github: WebhookSourceInfo,
    #[serde(default)]
    pub jira: WebhookSourceInfo,
}

/// Response of `POST /api/setup/webhook-secret/{source}`: the only time the value is sent.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhookSecretResponse {
    #[serde(default)]
    pub secret: String,
}

/// Response of `POST /api/setup/webhook-test/{source}`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WebhookTestResponse {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub message: cockpit_i18n::Msg,
}
