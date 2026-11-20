//! Agent drafts and GitHub: nothing goes out before "send", and then only what you approved.

use super::*;
use cockpit_core::{Action, Actor, AgentReport, DiffSide, PrKey};

const RUN: &str = "prreview-a~x-7";

fn apply(state: &AppState, action: Action, actor: Actor) -> bool {
    state
        .inner
        .live
        .apply(&state.inner.db, action, actor, crate::app::now_ms())
        .is_ok()
}

fn report(state: &AppState, r: AgentReport) -> bool {
    apply(
        state,
        Action::ReportAgent {
            run: RUN.into(),
            report: r,
        },
        Actor::Agent,
    )
}

fn draft(line: u32, side: DiffSide, text: &str, suggestion: Option<&str>) -> AgentReport {
    AgentReport::ReviewComment {
        path: "a.rs".into(),
        line,
        start_line: None,
        side,
        text: text.into(),
        suggestion: suggestion.map(str::to_string),
    }
}

#[tokio::test]
async fn only_approved_drafts_go_out_in_one_review_with_event_comment() {
    let g = github().await;
    // Exactly one review, the verdict is COMMENT, only the approved draft is in it.
    Mock::given(method("POST"))
        .and(path("/repos/a/x/pulls/7/reviews"))
        .and(wiremock::matchers::body_json(json!({
            "event": "COMMENT", "commit_id": "s1",
            "comments": [{"path": "a.rs", "line": 2, "side": "RIGHT",
                          "body": "Name prüfen\n\n```suggestion\n    besser();\n```"}]
        })))
        .respond_with(ok(json!({"id": 1})))
        .expect(1)
        .mount(&g)
        .await;
    let state = state(sources(&g.uri()), true);
    sync(&state).await;
    assert!(apply(
        &state,
        Action::StartPrReview {
            review: PrKey::new("a/x", 7),
            language: None,
            repo_language: None
        },
        Actor::User
    ));
    assert!(report(
        &state,
        AgentReport::Started {
            detail: "docker".into()
        }
    ));
    assert!(report(
        &state,
        draft(2, DiffSide::Right, "Name prüfen", Some("    besser();"))
    ));
    assert!(report(
        &state,
        draft(2, DiffSide::Left, "Alte Zeile unnötig", None)
    ));
    // A draft for a line that is not in the diff never becomes a draft.
    assert!(!report(
        &state,
        draft(40, DiffSide::Right, "gibt es nicht", None)
    ));
    assert!(report(
        &state,
        AgentReport::Done {
            summary: "2 Kommentare als Entwurf vorbereitet.".into()
        }
    ));

    // Drafts alone send nothing, and sending is refused while one is undecided.
    let posts =
        |reqs: Vec<wiremock::Request>| reqs.iter().filter(|r| r.method.as_str() == "POST").count();
    assert_eq!(posts(g.received_requests().await.unwrap()), 0);
    let (s, _) = action(&state, json!({"SendReview": {"review": 7, "comment": ""}})).await;
    assert_ne!(s, StatusCode::OK, "undecided drafts block sending");
    assert_eq!(posts(g.received_requests().await.unwrap()), 0);

    let (ws, _) = snapshot(&state).await;
    let ids: Vec<String> = ws
        .review(7)
        .unwrap()
        .suggestions
        .iter()
        .map(|s| s.id.clone())
        .collect();
    assert_eq!(ids.len(), 2);
    action(
        &state,
        json!({"Decide": {"review": 7, "suggestion": ids[0], "decision": "Approved"}}),
    )
    .await;
    action(
        &state,
        json!({"Decide": {"review": 7, "suggestion": ids[1], "decision": "Declined"}}),
    )
    .await;
    let (s, b) = action(&state, json!({"SendReview": {"review": 7, "comment": ""}})).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    // A second click does not send a second review (the mock expects exactly one).
    let (s, _) = action(&state, json!({"SendReview": {"review": 7, "comment": ""}})).await;
    assert_ne!(s, StatusCode::OK);
}
