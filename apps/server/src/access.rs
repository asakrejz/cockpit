//! Who may call the routes under `/api` – protection against foreign web pages.
//!
//! The backend only listens on `127.0.0.1`, but that alone does not protect against a web page
//! in the same browser. Therefore the following applies to all `/api` routes:
//!
//! * The peer is a loopback address.
//! * The `Host` header is `127.0.0.1:<port>` or `localhost:<port>` (against DNS rebinding:
//!   there it carries the foreign name).
//!
//! Only for development with `trunk serve` there is a second allowed port
//! (`COCKPIT_DEV_PORT`, only by environment variable): Trunk forwards `/api`, sends the `Host`
//! without a port (`127.0.0.1`) and leaves the `Origin` of the development page as it is. Only then do
//! both apply; without the variable only the own port applies.
//!
//! For everything that writes (not GET/HEAD), additionally:
//!
//! * `Origin` is missing or is exactly one of the two own origins (against foreign pages
//!   that send by form or `fetch`).
//! * `Content-Type` is `application/json`. A form cannot set that, `fetch` with
//!   this type triggers a preflight for a foreign origin, which we do not allow.

use std::net::{IpAddr, SocketAddr};

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cockpit_core::api::ErrorBody;
use cockpit_i18n::msg;

/// Checks a request. `peer` is the address of the peer if known – if it is missing,
/// the request is rejected.
pub fn check(
    peer: Option<IpAddr>,
    method: &Method,
    head: &HeaderMap,
    ports: &[u16],
    dev: bool,
) -> Result<(), (StatusCode, cockpit_i18n::Msg)> {
    if !peer.is_some_and(|ip| ip.is_loopback()) {
        return Err((StatusCode::FORBIDDEN, msg!(ServerAccessLocalOnly)));
    }
    let host = head.get(HOST).and_then(|h| h.to_str().ok());
    let host_ok = host.is_some_and(|h| {
        let h = h.to_ascii_lowercase();
        // Trunk forwards the host without a port; that only applies in development mode.
        (dev && (h == "127.0.0.1" || h == "localhost"))
            || ports
                .iter()
                .any(|p| h == format!("127.0.0.1:{p}") || h == format!("localhost:{p}"))
    });
    if !host_ok {
        return Err((StatusCode::FORBIDDEN, msg!(ServerAccessUnexpectedHost)));
    }
    if matches!(*method, Method::GET | Method::HEAD) {
        return Ok(());
    }
    if let Some(origin) = head.get(ORIGIN) {
        let origin = origin.to_str().unwrap_or("").to_ascii_lowercase();
        let own = ports.iter().any(|p| {
            origin == format!("http://127.0.0.1:{p}") || origin == format!("http://localhost:{p}")
        });
        if !own {
            return Err((StatusCode::FORBIDDEN, msg!(ServerAccessForeignOrigin)));
        }
    }
    let is_json = head
        .get(CONTENT_TYPE)
        .and_then(|c| c.to_str().ok())
        .is_some_and(|c| {
            c.split(';')
                .next()
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"))
        });
    if !is_json {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            msg!(ServerAccessExpectJson),
        ));
    }
    Ok(())
}

/// How strict the check is.
#[derive(Clone)]
pub struct Access {
    /// The port on which the server listens and – only on explicit request
    /// (`COCKPIT_DEV_PORT`) – that of `trunk serve`.
    pub ports: Vec<u16>,
    /// Development mode: host also without a port.
    pub dev: bool,
}

/// Middleware for the `/api` part of the router.
pub async fn protect(State(access): State<Access>, request: Request, next: Next) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    match check(
        peer,
        request.method(),
        request.headers(),
        &access.ports,
        access.dev,
    ) {
        Ok(()) => next.run(request).await,
        Err((status, message)) => (status, Json(ErrorBody::coded(message))).into_response(),
    }
}

#[cfg(test)]
mod tests;
