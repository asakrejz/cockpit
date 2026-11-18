//! `POST /api/translate`: translates a text from Jira or GitHub with the model endpoint of the
//! agent. Only on request (a button), never by itself. The text goes to the model service; the key
//! never leaves the backend. Translations are kept in a small cache, so the same text is not sent
//! twice.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cockpit_core::api::{TranslateRequest, TranslateResponse};
use cockpit_i18n::msg;
use cockpit_integrations::translate::{self, TranslateError};

use crate::app::{AppState, error_msg};

/// So many translations are kept; when the cache is full it starts over.
const CACHE_SIZE: usize = 256;

fn cache() -> &'static Mutex<HashMap<u64, String>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The cache key: model, language and text.
fn cache_key(model: &str, request: &TranslateRequest) -> u64 {
    let mut hasher = DefaultHasher::new();
    model.hash(&mut hasher);
    request.to.hash(&mut hasher);
    request.text.hash(&mut hasher);
    hasher.finish()
}

fn cached(key: u64) -> Option<String> {
    cache().lock().ok()?.get(&key).cloned()
}

fn remember(key: u64, text: &str) {
    if let Ok(mut cache) = cache().lock() {
        if cache.len() >= CACHE_SIZE {
            cache.clear();
        }
        cache.insert(key, text.to_string());
    }
}

pub(crate) async fn translate(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<TranslateRequest>(&body) else {
        return error_msg(StatusCode::BAD_REQUEST, msg!(ServerMiscTranslateUnreadable));
    };
    if request.text.trim().is_empty() {
        return Json(TranslateResponse {
            text: String::new(),
        })
        .into_response();
    }
    let model = state
        .agent_choice()
        .endpoint
        .and_then(|e| e.model)
        .unwrap_or_default();
    let key = cache_key(&model, &request);
    if let Some(text) = cached(key) {
        return Json(TranslateResponse { text }).into_response();
    }
    let secret = match state.endpoint_key().await {
        Ok(k) => k,
        Err(answer) => return *answer,
    };
    match translate::translate(
        &state.endpoint_target(),
        secret.as_ref(),
        Some(&model),
        &request.text,
        request.to,
    )
    .await
    {
        Ok(text) => {
            remember(key, &text);
            Json(TranslateResponse { text }).into_response()
        }
        Err(e) => {
            // The reason is plain; neither the text nor the key are logged.
            tracing::warn!("Translation failed: {e:?}");
            let status = match e {
                TranslateError::NoKey | TranslateError::NoModel | TranslateError::TooLong => {
                    StatusCode::UNPROCESSABLE_ENTITY
                }
                _ => StatusCode::BAD_GATEWAY,
            };
            error_msg(status, e.msg())
        }
    }
}
