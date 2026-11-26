//! The setup assistant: the current values, configuration, keys, disconnecting and the
//! webhook secrets (`/api/setup`, `/api/config`, `/api/secrets/…`, `/api/oauth/…`).

use cockpit_core::api::{
    ConfigChange, ConfigResponse, KeyInput, KeyResponse, RevokeResponse, Setup,
    WebhookSecretResponse, WebhookTestResponse,
};
use feature_ui::t;
use gloo_net::http::Request;

use super::{CallError, rejection};

/// "Disconnect": revokes the sign-in (`POST /api/oauth/{service}/revoke`). `service`:
/// `github` or `atlassian`.
pub async fn post_revoke(service: &str) -> Result<RevokeResponse, CallError> {
    let request = Request::post(&format!("/api/oauth/{service}/revoke"))
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerDisconnectNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<RevokeResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// The current, non-secret values for the assistant (`GET /api/setup`).
pub async fn fetch_setup() -> Result<Setup, CallError> {
    let resp = Request::get("/api/setup")
        .send()
        .await
        .map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<Setup>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Writes client IDs, Jira, repos or provider and endpoint to `config.toml` (`PUT /api/config`).
pub async fn put_config(change: &ConfigChange) -> Result<ConfigResponse, CallError> {
    let request = Request::put("/api/config")
        .header("Content-Type", "application/json")
        .json(change)
        .map_err(|_| CallError::Rejected(t!(ServerInputsNotSent)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<ConfigResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Stores a key in the .env file of the server (`PUT /api/secrets/{name}`). The answer only says "set".
pub async fn put_key(name: &str, value: String) -> Result<KeyResponse, CallError> {
    let request = Request::put(&format!("/api/secrets/{name}"))
        .header("Content-Type", "application/json")
        .json(&KeyInput { value })
        .map_err(|_| CallError::Rejected(t!(ServerInputNotSent)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<KeyResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Deletes a token from the .env file (`DELETE /api/secrets/{name}`), "Disconnect".
pub async fn delete_key(name: &str) -> Result<KeyResponse, CallError> {
    let request = Request::delete(&format!("/api/secrets/{name}"))
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerDisconnectNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<KeyResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Generates a new webhook secret on the server (`POST /api/setup/webhook-secret/{source}`).
/// The answer carries the value once, so it can be pasted into GitHub or Jira.
pub async fn post_webhook_secret(source: &str) -> Result<WebhookSecretResponse, CallError> {
    let request = Request::post(&format!("/api/setup/webhook-secret/{source}"))
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerRequestNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<WebhookSecretResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}

/// Sends a signed test request from the server to itself (`POST /api/setup/webhook-test/{source}`).
pub async fn post_webhook_test(source: &str) -> Result<WebhookTestResponse, CallError> {
    let request = Request::post(&format!("/api/setup/webhook-test/{source}"))
        .header("Content-Type", "application/json")
        .build()
        .map_err(|_| CallError::Rejected(t!(ServerTestNotStarted)))?;
    let resp = request.send().await.map_err(|_| CallError::Unreachable)?;
    if !resp.ok() {
        return Err(rejection(resp).await);
    }
    resp.json::<WebhookTestResponse>()
        .await
        .map_err(|_| CallError::Rejected(t!(ServerGarbled)))
}
