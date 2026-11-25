//! The HTTP handlers of the API.

use super::*;

/// `GET /api/agent/check`: only calls `GET <url>/v1/models` with the
/// key and reports whether the endpoint is reachable, accepts the key and offers the model.
/// Never the key.
pub(super) async fn endpoint_check(State(state): State<AppState>) -> Response {
    let choice = state.agent_choice();
    let Some(e) = choice.endpoint.clone() else {
        // Without an entry there is nothing to check (not even "direct": first the person chooses a model).
        return Json(cockpit_core::api::EndpointCheck {
            hint: Some(cockpit_i18n::msg!(ServerMiscEndpointNotSetUp)),
            ..Default::default()
        })
        .into_response();
    };
    let key = match state.endpoint_key().await {
        Ok(k) => k,
        Err(answer) => return *answer,
    };
    Json(
        cockpit_integrations::endpoint::verify(
            &state.endpoint_target(),
            key.as_ref(),
            e.model.as_deref(),
        )
        .await,
    )
    .into_response()
}

/// `GET /api/agent/models`: the model IDs of the endpoint (IDs only, sorted) for the drop-down.
/// The key comes from the .env file and never leaves the backend.
pub(super) async fn agent_models(State(state): State<AppState>) -> Response {
    let key = match state.endpoint_key().await {
        Ok(k) => k,
        Err(answer) => return *answer,
    };
    Json(cockpit_integrations::endpoint::models_load(&state.endpoint_target(), key.as_ref()).await)
        .into_response()
}

/// `GET /api/agent/image`: state of "Image bauen" (running, present, last lines, error).
pub(super) async fn agent_image_read(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.image_state()).await {
        Ok(b) => Json(b).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/agent/image`: builds the agent image in the background (`docker build` or `podman build`).
/// The progress is in `GET /api/agent/image`.
pub(super) async fn agent_image_build(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || state.image_build()).await {
        Ok(Ok(b)) => Json(b).into_response(),
        Ok(Err(m)) => error_msg(StatusCode::UNPROCESSABLE_ENTITY, m),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `GET /api/agent/check/container`: does a
/// container reach the endpoint (check mode of the image)?
pub(super) async fn endpoint_check_container(State(state): State<AppState>) -> Response {
    Json(state.container_check().await).into_response()
}

/// `GET /api/agent/pull-request?run=<key>`: preview of the pull request for the branch of a
/// finished real run. Creates nothing.
pub(super) async fn pull_request_preview(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(run) = q.get("run").filter(|r| !r.is_empty()) else {
        return error_msg(
            StatusCode::BAD_REQUEST,
            cockpit_i18n::msg!(ServerMiscRunMissing),
        );
    };
    let repo = q.get("repo").map_or("", String::as_str);
    match state.pull_request_preview(run, repo).await {
        Ok(p) => Json(p).into_response(),
        Err(e) => action_error(e),
    }
}

pub(super) async fn api_unknown() -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody::new(cockpit_i18n::msg!(ServerMiscUnknownAddress))),
    )
}

/// An error answer with the message as code and parameters (the UI shows it in its language).
pub(crate) fn error_msg(status: StatusCode, msg: cockpit_i18n::Msg) -> Response {
    (status, Json(ErrorBody::new(msg))).into_response()
}

/// `GET /api/workspace`: the current state, shape of `cockpit_core::persist` plus `revision`.
pub(super) async fn workspace(State(state): State<AppState>) -> Response {
    (
        [(CONTENT_TYPE, "application/json")],
        state.inner.live.snapshot(),
    )
        .into_response()
}

/// `POST /api/actions`: applies the action as "Du" with the time of the server. If it acts
/// to the outside (Jira, GitHub), the call to the service goes first; if it fails, nothing
/// changes. We read the body ourselves so that an unknown action gets a German message.
pub(super) async fn action(State(state): State<AppState>, body: Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<ActionRequest>(&body) else {
        return error_msg(
            StatusCode::BAD_REQUEST,
            cockpit_i18n::msg!(ServerMiscActionUnknown),
        );
    };
    // Another Jira project: sync right away so that its tickets arrive. The result
    // reaches the app through the event stream.
    let sprint_before = state.inner.live.copy().inbox_filter.sprint;
    let project_switched = matches!(
        request.action,
        cockpit_core::Action::SelectJiraProject { .. }
    );
    let filter_action = matches!(request.action, cockpit_core::Action::SetInboxFilter { .. });
    match state.action_run(request.action).await {
        Ok(answer) => {
            // Another sprint view asks Jira for other tickets (all open ones, the backlog …).
            let sprint_switched =
                filter_action && state.inner.live.copy().inbox_filter.sprint != sprint_before;
            if project_switched || sprint_switched {
                let s = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = s.sync().await {
                        tracing::warn!("Sync after project change: {e:?}");
                    }
                });
            }
            Json(answer).into_response()
        }
        Err(e) => action_error(e),
    }
}

/// `POST /api/sync`: "jetzt abgleichen".
pub(super) async fn sync(State(state): State<AppState>) -> Response {
    match state.sync().await {
        Ok(answer) => Json(answer).into_response(),
        Err(e) => action_error(e),
    }
}

/// `GET /api/jira/projects`: the projects for the selection in the inbox.
pub(super) async fn jira_projects(State(state): State<AppState>) -> Response {
    match state.jira_projects().await {
        Ok(answer) => Json(answer).into_response(),
        Err(e) => action_error(e),
    }
}

/// `GET /api/jira/assignable?project=…&query=…`: people the tickets of the project can be
/// assigned to (read only).
pub(super) async fn jira_assignable(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let get = |k: &str| q.get(k).map(String::as_str).unwrap_or_default();
    match state.jira_assignable(get("project"), get("query")).await {
        Ok(answer) => Json(answer).into_response(),
        Err(e) => action_error(e),
    }
}

/// `GET /api/github/assignable?repo=…&query=…`: people the issues of the repo can be assigned to
/// (read only).
pub(super) async fn github_assignable(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let get = |k: &str| q.get(k).map(String::as_str).unwrap_or_default();
    match state.github_assignable(get("repo"), get("query")).await {
        Ok(answer) => Json(answer).into_response(),
        Err(e) => action_error(e),
    }
}

pub(super) fn action_error(e: ActionFailure) -> Response {
    match e {
        // All rejections by the rules are 422 – without listing individual errors.
        ActionFailure::Rule(e) => error_msg(StatusCode::UNPROCESSABLE_ENTITY, e.msg()),
        ActionFailure::Plan(m) => error_msg(StatusCode::UNPROCESSABLE_ENTITY, m),
        // The service has rejected or is not reachable.
        ActionFailure::Service(m) => error_msg(StatusCode::BAD_GATEWAY, m),
        ActionFailure::Save(m) => {
            tracing::error!("Workspace not saved: {m}");
            error_msg(
                StatusCode::INTERNAL_SERVER_ERROR,
                cockpit_i18n::msg!(ServerAgentSaveFailed),
            )
        }
        ActionFailure::Intern => error_msg(
            StatusCode::INTERNAL_SERVER_ERROR,
            cockpit_i18n::msg!(ServerAgentInternal),
        ),
    }
}

/// `GET /api/events`: server-sent events. First the current revision, then one event
/// per applied action. Whoever reads too slowly gets the current revision and reloads.
pub(super) async fn events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let (rx, first) = state.inner.live.subscribe();
    let more = BroadcastStream::new(rx).map(move |event| {
        event.unwrap_or_else(|_| ServerEvent {
            revision: state.inner.live.revision(),
            entries: Vec::new(),
        })
    });
    let strom = tokio_stream::once(first)
        .chain(more)
        .map(|e| Ok(Event::default().json_data(&e).unwrap_or_default()));
    Sse::new(strom).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}
