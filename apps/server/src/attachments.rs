//! `GET /api/jira/attachment/{id}` and `GET /api/jira/attachment/{id}/thumbnail`: the attachments
//! of Jira tickets, read through the key of the backend. Jira's attachment addresses need a
//! sign-in, so the browser never gets them – and never the key.
//!
//! Rules:
//!
//! * like every `/api` route: only from the same computer, with a checked `Host`;
//! * only attachments of tickets that are in the workspace (the ID is looked up there, never
//!   passed on blindly; the address at Jira is built from the digits of the ID only);
//! * a size limit; larger files answer 413;
//! * only images whose first bytes really are PNG, JPEG, GIF or WebP are shown inline. Everything
//!   else (also SVG) is a download with `Content-Disposition: attachment`;
//! * `nosniff` and a `sandbox` policy on every answer; no cookies, no key in the answer;
//! * a small cache in memory with an upper bound in bytes.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderName,
};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use cockpit_core::Attachment;
use cockpit_i18n::msg;
use cockpit_integrations::{Cause, Error};

use crate::app::{AppState, error_msg};

/// The largest attachment (and so the largest picture) the proxy hands out.
pub const MAX_BYTES: usize = 10 * 1024 * 1024;
/// The largest thumbnail.
pub const MAX_THUMB_BYTES: usize = 2 * 1024 * 1024;
/// The cache holds at most this many bytes (the oldest entry goes first).
pub const CACHE_BYTES: usize = 32 * 1024 * 1024;

/// A file that was read, with the type the app serves it as.
#[derive(Clone)]
struct Stored {
    bytes: Arc<Vec<u8>>,
    /// Set if the bytes are a picture the browser may show inline.
    image: Option<&'static str>,
}

/// Attachment ID and whether it is the thumbnail.
type Key = (String, bool);

/// The entries (oldest first) and how many bytes they hold together.
type Entries = (VecDeque<(Key, Stored)>, usize);

/// In-memory cache with an upper bound in bytes.
#[derive(Default)]
pub struct Cache {
    inner: Mutex<Entries>,
}

impl Cache {
    fn get(&self, key: &Key) -> Option<Stored> {
        let guard = self.inner.lock().ok()?;
        guard
            .0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    fn put(&self, key: Key, value: Stored) {
        let size = value.bytes.len();
        if size > CACHE_BYTES {
            return;
        }
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        guard.0.retain(|(k, _)| *k != key);
        guard.1 = guard.0.iter().map(|(_, v)| v.bytes.len()).sum();
        while guard.1 + size > CACHE_BYTES {
            let Some((_, old)) = guard.0.pop_front() else {
                break;
            };
            guard.1 -= old.bytes.len();
        }
        guard.1 += size;
        guard.0.push_back((key, value));
    }

    /// How many bytes are held (tests).
    #[cfg(test)]
    pub fn size(&self) -> usize {
        self.inner.lock().map_or(0, |g| g.1)
    }
}

/// The type of a picture from its first bytes; `None` if it is none of the allowed ones.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// A file name that is safe in a header: no quotes, slashes, control characters or non-ASCII.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "attachment".into()
    } else {
        cleaned
    }
}

impl AppState {
    /// The attachment record of a ticket in the workspace, if the ID belongs to one.
    fn attachment_known(&self, id: &str) -> Option<Attachment> {
        let ws = self.inner.live.copy();
        ws.items
            .iter()
            .filter_map(|i| i.details.as_ref())
            .flat_map(|d| d.attachments.iter())
            .find(|a| a.id == id)
            .cloned()
    }
}

pub(crate) async fn content(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    serve(state, id, false).await
}

pub(crate) async fn thumbnail(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    serve(state, id, true).await
}

async fn serve(state: AppState, id: String, thumb: bool) -> Response {
    let not_found = || error_msg(StatusCode::NOT_FOUND, msg!(IntegrationsNotFound));
    // Digits only, and only an attachment of a ticket we know.
    if id.is_empty() || id.len() > 20 || !id.bytes().all(|c| c.is_ascii_digit()) {
        return not_found();
    }
    let Some(record) = state.attachment_known(&id) else {
        return not_found();
    };
    // A thumbnail only exists for pictures that Jira made one for.
    if thumb && !(record.thumbnail && record.is_image()) {
        return not_found();
    }
    let key = (id.clone(), thumb);
    let stored = match state.inner.attachments.get(&key) {
        Some(s) => s,
        None => match fetch(&state, &id, thumb).await {
            Ok(s) => {
                state.inner.attachments.put(key, s.clone());
                s
            }
            Err(e) => return failure(e),
        },
    };
    respond(stored, &record, thumb)
}

/// The picture of the sample data, built into the program.
const DEMO_IMAGE: &[u8] = include_bytes!("attachments/placeholder.png");

async fn fetch(state: &AppState, id: &str, thumb: bool) -> Result<Stored, Error> {
    state.oauth_refresh().await;
    let s = state.clone();
    let client = tokio::task::spawn_blocking(move || s.jira_client())
        .await
        .map_err(|_| Error::Config("internal".into()))?;
    // Without Jira only the picture of the sample data exists – it never stands in for a
    // real attachment.
    let Some(client) = client else {
        if id == cockpit_core::details::DEMO_IMAGE_ID {
            return Ok(Stored {
                bytes: Arc::new(DEMO_IMAGE.to_vec()),
                image: sniff_image(DEMO_IMAGE),
            });
        }
        return Err(Error::NotSignedIn);
    };
    let client = client?;
    let max = if thumb { MAX_THUMB_BYTES } else { MAX_BYTES };
    let download = client.attachment(id, thumb, max).await?;
    let image = sniff_image(&download.bytes);
    Ok(Stored {
        bytes: Arc::new(download.bytes),
        image,
    })
}

fn failure(e: Error) -> Response {
    tracing::warn!("Jira attachment: {e}");
    let status = match e.cause() {
        Cause::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        Cause::NotFound => StatusCode::NOT_FOUND,
        // Also a refused key: the request of the browser was fine, the answer of Jira was not.
        _ => StatusCode::BAD_GATEWAY,
    };
    error_msg(status, e.msg())
}

fn respond(stored: Stored, record: &Attachment, thumb: bool) -> Response {
    let nosniff = (
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    let policy = (
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    let cache = (
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    let bytes = stored.bytes.as_ref().clone();
    // Inline only if the record says "picture" *and* the bytes agree (a thumbnail has no
    // record of its own: the bytes decide).
    match stored.image {
        Some(kind) if thumb || record.is_image() => (
            [
                (CONTENT_TYPE, HeaderValue::from_static(kind)),
                (CONTENT_DISPOSITION, HeaderValue::from_static("inline")),
                nosniff,
                policy,
                cache,
            ],
            bytes,
        )
            .into_response(),
        _ => {
            let name = safe_file_name(&record.name);
            let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
                .unwrap_or_else(|_| HeaderValue::from_static("attachment"));
            (
                [
                    (
                        CONTENT_TYPE,
                        HeaderValue::from_static("application/octet-stream"),
                    ),
                    (CONTENT_DISPOSITION, disposition),
                    nosniff,
                    policy,
                    cache,
                ],
                bytes,
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests;
