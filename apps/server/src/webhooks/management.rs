//! Webhook management for the setup page: new secret and the local test.

use super::*;

pub(super) fn source_from(text: &str) -> Option<Source> {
    match text {
        "github" => Some(Source::GitHub),
        "jira" => Some(Source::Jira),
        _ => None,
    }
}

/// `POST /api/setup/webhook-secret/{source}`: generates a new random secret, stores it in the
/// `.env` file (replacing an old one) and returns it **this once** so it can be pasted into
/// GitHub or Jira. It cannot be read again.
pub async fn secret_new(
    State(state): State<AppState>,
    UrlPath(source): UrlPath<String>,
) -> Response {
    let Some(source) = source_from(&source) else {
        return error(StatusCode::NOT_FOUND, "Unbekannte Quelle");
    };
    let secret = match secret_generate() {
        Ok(s) => s,
        Err(m) => return error(StatusCode::INTERNAL_SERVER_ERROR, m),
    };
    let store = state.inner.secrets.clone();
    let value = Secret::new(secret.clone());
    match tokio::task::spawn_blocking(move || store.set(secret_name(source), &value)).await {
        Ok(Ok(())) => {
            tracing::info!("Webhook secret for {} generated", name(source));
            Json(WebhookSecretResponse { secret }).into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!("Webhook secret for {} not stored: {e}", name(source));
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Die .env-Datei im Projektordner ließ sich nicht beschreiben. Prüfe die Rechte von Ordner und Datei.",
            )
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/setup/webhook-test/{source}`: sends a signed test request to this server over
/// loopback and reports what came back. Calls no external service.
pub async fn test(State(state): State<AppState>, UrlPath(source): UrlPath<String>) -> Response {
    let Some(source) = source_from(&source) else {
        return error(StatusCode::NOT_FOUND, "Unbekannte Quelle");
    };
    Json(state.webhook_test(source).await).into_response()
}

pub(super) fn test_answer(ok: bool, message: &str) -> WebhookTestResponse {
    WebhookTestResponse {
        ok,
        message: message.into(),
    }
}

impl AppState {
    pub(super) async fn webhook_test(&self, source: Source) -> WebhookTestResponse {
        if !self.inner.webhooks.settings().enabled {
            return test_answer(
                false,
                "Webhooks sind noch nicht aktiviert. Schalte sie oben ein.",
            );
        }
        let s = self.clone();
        let secret = tokio::task::spawn_blocking(move || s.token(secret_name(source)))
            .await
            .ok()
            .flatten();
        let Some(secret) = secret else {
            return test_answer(false, "Es gibt noch kein Secret. Erzeuge zuerst eines.");
        };
        // The body differs every time so that the replay check does not swallow a second test.
        let nonce = format!("{}-{}", now_ms(), std::process::id());
        let (path, body, headers) = match source {
            Source::GitHub => (
                GITHUB_PATH,
                format!(r#"{{"zen":"cockpit local test","nonce":"{nonce}"}}"#),
                "X-GitHub-Event: ping\r\n",
            ),
            Source::Jira => (
                JIRA_PATH,
                format!(r#"{{"webhookEvent":"cockpit:test","nonce":"{nonce}"}}"#),
                "",
            ),
        };
        let signature = signature_header(secret.expose().as_bytes(), body.as_bytes());
        let signature_name = match source {
            Source::GitHub => "X-Hub-Signature-256",
            Source::Jira => "X-Hub-Signature",
        };
        let port = self.inner.port;
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{signature_name}: {signature}\r\n{headers}X-Cockpit-Test: 1\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        match local_status(port, &request).await {
            Some(200) => test_answer(
                true,
                "Das Secret stimmt und die Route antwortet. Das ist ein lokaler Test: ob GitHub bzw. Jira dich über den Tunnel erreicht, zeigt erst „Zuletzt empfangen“ nach einer echten Zustellung.",
            ),
            Some(401) => test_answer(
                false,
                "Die Signatur wurde abgelehnt – das Secret passt nicht.",
            ),
            Some(404) => test_answer(
                false,
                "Die Route antwortet nicht. Sind Webhooks aktiviert und ein Secret gesetzt?",
            ),
            Some(429) => test_answer(
                false,
                "Zu viele Anfragen in kurzer Zeit. Warte eine Minute.",
            ),
            Some(_) => test_answer(false, "Der Server hat den Test abgelehnt."),
            None => test_answer(false, "Der lokale Server hat nicht geantwortet."),
        }
    }
}

/// Sends a raw HTTP/1.1 request to `127.0.0.1:port` and returns the status code.
pub(super) async fn local_status(port: u16, request: &str) -> Option<u16> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let call = async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .ok()?;
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut answer = Vec::new();
        let mut buf = [0u8; 512];
        // The status line is enough.
        while !answer.windows(2).any(|w| w == b"\r\n") && answer.len() < 4096 {
            let n = stream.read(&mut buf).await.ok()?;
            if n == 0 {
                break;
            }
            answer.extend_from_slice(&buf[..n]);
        }
        let line = String::from_utf8_lossy(&answer);
        line.lines().next()?.split_whitespace().nth(1)?.parse().ok()
    };
    tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .ok()
        .flatten()
}
