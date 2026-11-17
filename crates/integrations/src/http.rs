//! Shared fetch: translate status into [`Error`], read JSON.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use reqwest::{RequestBuilder, StatusCode};

use crate::Error;

pub(crate) fn client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .user_agent(concat!("cockpit-server/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| Error::Config(e.without_url().to_string()))
}

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn header<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

/// How long should we pause? `Retry-After` (seconds) wins; otherwise the time
/// at which the quota comes back (`X-RateLimit-Reset`, seconds since 1970).
pub(crate) fn wait(h: &HeaderMap, now_s: u64) -> Option<u64> {
    if let Some(s) = header(h, "retry-after").and_then(|v| v.trim().parse::<u64>().ok()) {
        return Some(s);
    }
    let reset = header(h, "x-ratelimit-reset")?.trim().parse::<u64>().ok()?;
    Some(reset.saturating_sub(now_s))
}

/// The service's message from an error body: GitHub (`message`), Jira
/// (`errorMessages`, `errors`). Shortened so nothing huge ends up in the UI.
pub(crate) fn service_message(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let mut parts: Vec<String> = Vec::new();
    // OAuth endpoints (`error_description`, otherwise `error`).
    if let Some(m) = json["error_description"]
        .as_str()
        .or_else(|| json["error"].as_str())
    {
        parts.push(m.to_string());
    }
    if let Some(m) = json["message"].as_str() {
        parts.push(m.to_string());
    }
    for m in json["errorMessages"].as_array().into_iter().flatten() {
        if let Some(m) = m.as_str() {
            parts.push(m.to_string());
        }
    }
    if let Some(error) = json["errors"].as_object() {
        for v in error.values() {
            if let Some(m) = v.as_str() {
                parts.push(m.to_string());
            }
        }
    }
    // GitHub: details are in `errors` for 422, as a list of objects/texts.
    for e in json["errors"].as_array().into_iter().flatten() {
        match e {
            serde_json::Value::String(s) => parts.push(s.clone()),
            other => {
                if let Some(m) = other["message"].as_str() {
                    parts.push(m.to_string());
                }
            }
        }
    }
    let text = parts.join("; ");
    let text: String = text.chars().take(240).collect();
    (!text.trim().is_empty()).then_some(text)
}

/// Translates an error response. Pure, so every case can be tested.
pub(crate) fn error_from(status: StatusCode, h: &HeaderMap, body: &str, now_s: u64) -> Error {
    let limit_empty = header(h, "x-ratelimit-remaining") == Some("0");
    let pause = header(h, "retry-after").is_some();
    match status {
        StatusCode::UNAUTHORIZED => Error::NotSignedIn,
        StatusCode::TOO_MANY_REQUESTS => Error::RateLimit {
            wait_s: wait(h, now_s),
        },
        StatusCode::FORBIDDEN if limit_empty || pause => Error::RateLimit {
            wait_s: wait(h, now_s),
        },
        StatusCode::FORBIDDEN => Error::NoPermission,
        StatusCode::NOT_FOUND => Error::NotFound,
        StatusCode::BAD_REQUEST
        | StatusCode::METHOD_NOT_ALLOWED
        | StatusCode::CONFLICT
        | StatusCode::UNPROCESSABLE_ENTITY => {
            Error::Rejected(service_message(body).unwrap_or_default())
        }
        s => Error::Status(s.as_u16()),
    }
}

/// Sends the request and returns the body of the successful response (also empty, e.g. 204).
pub(crate) async fn send(req: RequestBuilder) -> Result<String, Error> {
    let resp = req
        .send()
        .await
        .map_err(|e| Error::Network(e.without_url().to_string()))?;
    let status = resp.status();
    let headers = resp.headers().clone();
    let text = resp
        .text()
        .await
        .map_err(|e| Error::Network(e.without_url().to_string()))?;
    if !status.is_success() {
        return Err(error_from(status, &headers, &text, now_s()));
    }
    Ok(text)
}

/// Removes trailing slashes from a base address and checks the scheme.
pub(crate) fn base_url(url: &str) -> Result<String, Error> {
    let url = url.trim().trim_end_matches('/');
    if url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost")
    {
        Ok(url.to_string())
    } else {
        Err(Error::Config(format!(
            "Adresse muss mit https:// beginnen: {url}"
        )))
    }
}

#[cfg(test)]
mod tests;
