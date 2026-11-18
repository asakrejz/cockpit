//! The agent's model endpoint: connection test and model list with a single call,
//! `GET <url>/v1/models`.
//!
//! * A proxy or other compatible endpoint (LiteLLM and others): header `Authorization: Bearer <key>`.
//! * Directly at Anthropic (`https://api.anthropic.com/v1/models`): `x-api-key` and `anthropic-version`.
//! * Codex (OpenAI-compatible endpoint): always `Authorization: Bearer <key>`, directly at
//!   `https://api.openai.com/v1/models`.
//!
//! Only what the endpoint answers is reported (model IDs) – **never the key**. The call follows
//! no redirects, so the key does not end up elsewhere, has a timeout and reads only
//! a limited amount of the response.

use std::time::Duration;

use cockpit_core::api::{EndpointCheck, MAX_MODELS, ModelList};
use cockpit_i18n::{Msg, msg};
use reqwest::{StatusCode, redirect::Policy};

use crate::Secret;

/// The address of the Anthropic API (without trailing slash).
pub const ANTHROPIC_API: &str = "https://api.anthropic.com";
/// The address of the OpenAI API (without trailing slash).
pub const OPENAI_API: &str = "https://api.openai.com";
/// The API version Anthropic requires on every call.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// The hint names this many offered models when the desired one is missing.
const MAX_NAMES: usize = 8;
/// No more of the response is read (the list carries only IDs; real lists are far smaller).
const MAX_RESPONSE: usize = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Where the call goes and how it identifies itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Without trailing slash.
    pub base: String,
    /// Directly at the provider (no address entered).
    pub direct: bool,
    /// An OpenAI-compatible endpoint (Codex) instead of an Anthropic-compatible one.
    pub openai: bool,
}

impl Target {
    /// With an address a proxy or compatible endpoint, without an address directly at Anthropic.
    /// `direct_base` replaces the Anthropic address (for tests only).
    pub fn from(url: Option<&str>, direct_base: Option<&str>) -> Target {
        Target::for_provider(false, url, direct_base)
    }

    /// Like [`Target::from`], for the endpoint family of the tool: `openai` for Codex, otherwise Anthropic.
    pub fn for_provider(openai: bool, url: Option<&str>, direct_base: Option<&str>) -> Target {
        match url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) => Target {
                base: u.trim_end_matches('/').to_string(),
                direct: false,
                openai,
            },
            None => Target {
                base: direct_base
                    .unwrap_or(if openai { OPENAI_API } else { ANTHROPIC_API })
                    .trim_end_matches('/')
                    .to_string(),
                direct: true,
                openai,
            },
        }
    }

    fn vendor(&self) -> &'static str {
        if self.openai { "OpenAI" } else { "Anthropic" }
    }

    fn who(&self) -> Msg {
        if self.direct {
            Msg::raw(self.vendor())
        } else {
            msg!(IntegrationsEndpointAt, base = &self.base)
        }
    }

    fn key_advice(&self) -> Msg {
        msg!(IntegrationsEndpointKeyAdvice)
    }
}

/// Why the fetch brought no list.
enum Failed {
    NoKey,
    ClientNotStartable(String),
    Timeout,
    NotReachable,
    KeyRejected,
    Status(u16),
    NoDirectory,
}

impl Failed {
    fn reachable(&self) -> bool {
        matches!(
            self,
            Failed::KeyRejected | Failed::Status(_) | Failed::NoDirectory
        )
    }

    fn msg(&self, target: &Target) -> Msg {
        let who = target.who();
        match self {
            Failed::NoKey => msg!(IntegrationsEndpointNoKey, advice = target.key_advice()),
            Failed::ClientNotStartable(e) => {
                msg!(IntegrationsEndpointClientFailed, reason = Msg::raw(e))
            }
            Failed::Timeout => msg!(IntegrationsEndpointTimeout, who = who),
            Failed::NotReachable => {
                if target.direct {
                    let host = if target.openai {
                        "api.openai.com"
                    } else {
                        "api.anthropic.com"
                    };
                    msg!(
                        IntegrationsEndpointUnreachableDirect,
                        who = who,
                        host = host
                    )
                } else {
                    msg!(IntegrationsEndpointUnreachable, who = who)
                }
            }
            Failed::KeyRejected => msg!(
                IntegrationsEndpointKeyRejected,
                who = who,
                advice = target.key_advice()
            ),
            Failed::Status(s) => msg!(IntegrationsEndpointStatus, who = who, status = *s),
            Failed::NoDirectory => {
                if target.direct {
                    msg!(IntegrationsEndpointNotAListDirect, who = who)
                } else {
                    msg!(
                        IntegrationsEndpointNotAList,
                        base = &target.base,
                        vendor = target.vendor()
                    )
                }
            }
        }
    }
}

/// A usable model ID: not empty, not too long, without whitespace and control characters.
fn id_usable(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= 200
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Sorts the IDs (Claude models first, then alphabetically), removes duplicates and caps at [`MAX_MODELS`].
pub fn ids_order(mut ids: Vec<String>) -> Vec<String> {
    ids.sort_by(|a, b| {
        let claude = |s: &str| !s.to_lowercase().contains("claude");
        (claude(a), a).cmp(&(claude(b), b))
    });
    ids.dedup();
    ids.truncate(MAX_MODELS);
    ids
}

async fn fetch(target: &Target, key: Option<&Secret>) -> Result<Vec<String>, Failed> {
    let key = key.filter(|k| !k.is_empty()).ok_or(Failed::NoKey)?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("cockpit-server/", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .redirect(Policy::none())
        .build()
        .map_err(|e| Failed::ClientNotStartable(e.without_url().to_string()))?;
    let mut request = client.get(format!("{}/v1/models", target.base));
    request = if target.direct && !target.openai {
        // The list is paged; 1000 is the highest Anthropic allows.
        request
            .query(&[("limit", "1000")])
            .header("x-api-key", key.expose())
            .header("anthropic-version", ANTHROPIC_VERSION)
    } else {
        request.bearer_auth(key.expose())
    };
    let mut response = request.send().await.map_err(|e| {
        if e.is_timeout() {
            Failed::Timeout
        } else {
            Failed::NotReachable
        }
    })?;
    let status = response.status();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(Failed::KeyRejected);
    }
    if !status.is_success() {
        return Err(Failed::Status(status.as_u16()));
    }
    let mut body: Vec<u8> = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(part)) => {
                if body.len() + part.len() > MAX_RESPONSE {
                    return Err(Failed::NoDirectory);
                }
                body.extend_from_slice(&part);
            }
            Ok(None) => break,
            Err(e) if e.is_timeout() => return Err(Failed::Timeout),
            Err(_) => return Err(Failed::NoDirectory),
        }
    }
    let json: serde_json::Value = serde_json::from_slice(&body).map_err(|_| Failed::NoDirectory)?;
    let data = json["data"].as_array().ok_or(Failed::NoDirectory)?;
    Ok(ids_order(
        data.iter()
            .filter_map(|m| m["id"].as_str())
            .filter(|id| id_usable(id))
            .map(str::to_string)
            .collect(),
    ))
}

/// An answer that carries only a hint (German text and code).
trait Hinted {
    fn with(msg: Msg) -> Self;
}

impl Hinted for EndpointCheck {
    fn with(msg: Msg) -> Self {
        EndpointCheck {
            hint: Some(msg.de()),
            msg: Some(msg),
            ..Default::default()
        }
    }
}

impl Hinted for ModelList {
    fn with(msg: Msg) -> Self {
        ModelList {
            hint: Some(msg.de()),
            msg: Some(msg),
            ..Default::default()
        }
    }
}

fn hinted<T: Hinted>(msg: Msg) -> T {
    T::with(msg)
}

/// The endpoint's model IDs for the drop-down. Without a key nothing is called.
pub async fn models_load(target: &Target, key: Option<&Secret>) -> ModelList {
    match fetch(target, key).await {
        Ok(models) if models.is_empty() => ModelList {
            reachable: true,
            key_ok: true,
            ..hinted(msg!(IntegrationsEndpointNoModels, who = target.who()))
        },
        Ok(models) => ModelList {
            loaded: true,
            reachable: true,
            key_ok: true,
            models,
            hint: None,
            msg: None,
        },
        Err(m) => ModelList {
            reachable: m.reachable(),
            ..hinted(m.msg(target))
        },
    }
}

fn set_hint(mut check: EndpointCheck, msg: Msg) -> EndpointCheck {
    check.hint = Some(msg.de());
    check.msg = Some(msg);
    check
}

/// Checks the endpoint: reachable, key accepted, model present. `model` is the configured name.
pub async fn verify(target: &Target, key: Option<&Secret>, model: Option<&str>) -> EndpointCheck {
    let mut result = EndpointCheck {
        configured: true,
        ..Default::default()
    };
    let names = match fetch(target, key).await {
        Ok(n) => n,
        Err(m) => {
            result.reachable = m.reachable();
            return set_hint(result, m.msg(target));
        }
    };
    result.reachable = true;
    result.key_ok = true;
    match model {
        None => result = set_hint(result, msg!(IntegrationsEndpointNoModelSet)),
        Some(m) if names.iter().any(|n| n == m) => result.model_present = true,
        Some(m) => {
            let more = if names.len() > MAX_NAMES { ", …" } else { "" };
            let list: Vec<&str> = names.iter().take(MAX_NAMES).map(String::as_str).collect();
            let hint = if names.is_empty() {
                msg!(
                    IntegrationsEndpointModelMissingNone,
                    who = target.who(),
                    model = m
                )
            } else {
                msg!(
                    IntegrationsEndpointModelMissing,
                    who = target.who(),
                    model = m,
                    models = format!("{}{more}", list.join(", "))
                )
            };
            result = set_hint(result, hint);
        }
    }
    result
}

#[cfg(test)]
mod tests;
