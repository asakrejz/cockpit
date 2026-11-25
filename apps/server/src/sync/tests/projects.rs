//! GitHub Projects through the server: sync, writes that go to GitHub first, the flows with several
//! calls, and "Projekt anlegen". wiremock only; the answers follow the documented schema.

use super::*;
use crate::config::GitHubProjectsConfig;
use cockpit_core::{ProjectRef, TicketsFrom};
use wiremock::matchers::body_string_contains;

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(
            "../../../../../crates/integrations/tests/fixtures/",
            $name
        ))
    };
}

fn project_sources(api: &str, from: TicketsFrom) -> Sources {
    Sources {
        github_projects: Some(GitHubProjectsConfig {
            tickets_from: from,
            projects: vec![ProjectRef::new("acme", 3)],
            status_field: None,
            team_field: None,
        }),
        ..sources(api)
    }
}

async fn graphql(s: &MockServer, needle: &str, text: &str) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(needle))
        .respond_with(ResponseTemplate::new(200).set_body_raw(text.to_string(), "application/json"))
        .mount(s)
        .await;
}

async fn rest(s: &MockServer, verb: &str, p: &str, code: u16, body: &str) {
    Mock::given(method(verb))
        .and(path(p))
        .respond_with(
            ResponseTemplate::new(code).set_body_raw(body.to_string(), "application/json"),
        )
        .mount(s)
        .await;
}

/// GitHub with the sample project (two pages, details) and no pull requests.
async fn projects_server() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/user", ok(json!({"login": "mkeller"}))).await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ok(json!({"total_count": 0, "items": []})))
        .with_priority(200)
        .mount(&s)
        .await;
    get(&s, "/repos/a/x/pulls", ok(json!([]))).await;
    graphql(&s, "repositoryOwner", fixture!("gh_project_meta.json")).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("ProjectV2 {"))
        .and(body_string_contains("\"after\":null"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture!("gh_project_items_1.json"), "application/json"),
        )
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"after\":\"c1\""))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture!("gh_project_items_2.json"), "application/json"),
        )
        .mount(&s)
        .await;
    graphql(&s, "timelineItems", fixture!("gh_issue_details.json")).await;
    s
}

async fn graphql_calls(s: &MockServer, needle: &str) -> Vec<serde_json::Value> {
    s.received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/graphql")
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).ok())
        .filter(|b| b["query"].as_str().unwrap_or("").contains(needle))
        .collect()
}

#[tokio::test]
async fn the_sync_reads_project_tickets_and_health_says_so() {
    let g = projects_server().await;
    let st = state(project_sources(&g.uri(), TicketsFrom::Both), true);
    let (status, _) = sync(&st).await;
    assert_eq!(status, StatusCode::OK);
    let (ws, _) = snapshot(&st).await;
    assert!(ws.item_by_key("acme/app#12").is_some());
    assert!(ws.item_by_key("acme/web#15").is_some());
    assert_eq!(ws.sync.projects.len(), 1);
    let h = health(&st).await;
    assert_eq!(h["sync"]["github_projects"], "ok", "{h}");
    assert!(
        h["sync"]["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["source"] == "github_projects" && d["status"] == "ok")
    );
}

#[tokio::test]
async fn without_projects_in_the_choice_nothing_is_asked() {
    let g = projects_server().await;
    let st = state(project_sources(&g.uri(), TicketsFrom::Jira), true);
    sync(&st).await;
    assert!(
        graphql_calls(&g, "").await.is_empty(),
        "Jira alone: no GraphQL call"
    );
    assert_eq!(health(&st).await["sync"]["github_projects"], "off");
}

#[tokio::test]
async fn the_status_goes_to_github_first_and_then_to_the_workspace() {
    let g = projects_server().await;
    graphql(
        &g,
        "updateProjectV2ItemFieldValue",
        r#"{"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"PVTI_12"}}}}"#,
    )
    .await;
    let st = state(project_sources(&g.uri(), TicketsFrom::GitHubProjects), true);
    sync(&st).await;
    let (status, body) = action(
        &st,
        json!({"SetStatus": {"item": "ghi-acme-app-12", "status": "Review"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let calls = graphql_calls(&g, "updateProjectV2ItemFieldValue").await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0]["variables"]["value"],
        json!({"singleSelectOptionId": "s_rev"})
    );
    let (ws, _) = snapshot(&st).await;
    assert_eq!(
        ws.item("ghi-acme-app-12").unwrap().status,
        cockpit_core::TicketStatus::Review
    );
    assert_eq!(ws.log.last().unwrap().actor, Actor::User);
}

#[tokio::test]
async fn a_refused_write_changes_nothing_and_names_the_missing_right() {
    let g = projects_server().await;
    graphql(
        &g,
        "updateProjectV2ItemFieldValue",
        fixture!("gh_err_forbidden.json"),
    )
    .await;
    let st = state(project_sources(&g.uri(), TicketsFrom::Both), true);
    sync(&st).await;
    let (before, _) = snapshot(&st).await;
    let (status, body) = action(
        &st,
        json!({"SetStatus": {"item": "ghi-acme-app-12", "status": "Done"}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    // The token of the test is a classic one (`ghp_…`): the answer names the scope it lacks.
    assert!(
        crate::german::german(&body).contains("`project`")
            && crate::german::german(&body).contains("GitHub Projects ändern"),
        "{body}"
    );
    assert!(!body.contains(TOKEN));
    assert!(!body.contains(".."), "{body}");
    let (after, _) = snapshot(&st).await;
    assert_eq!(
        after.item("ghi-acme-app-12").unwrap().status,
        before.item("ghi-acme-app-12").unwrap().status
    );
}

#[tokio::test]
async fn a_comment_assign_and_label_go_to_the_issue() {
    let g = projects_server().await;
    rest(&g, "POST", "/repos/acme/app/issues/12/comments", 201, "{}").await;
    rest(&g, "PATCH", "/repos/acme/app/issues/12", 200, "{}").await;
    rest(&g, "POST", "/repos/acme/app/issues/12/labels", 200, "[]").await;
    let st = state(project_sources(&g.uri(), TicketsFrom::Both), true);
    sync(&st).await;
    for a in [
        json!({"Reply": {"item": "ghi-acme-app-12", "text": "Danke"}}),
        json!({"AssignTicket": {"item": "ghi-acme-app-12", "person": {"name": "tobias-lang", "initials": "TL"}}}),
        json!({"SetLabel": {"item": "ghi-acme-app-12", "label": "help wanted", "on": true}}),
        json!({"SetIssueClosed": {"item": "ghi-acme-app-12", "closed": true}}),
    ] {
        let (status, body) = action(&st, a).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (ws, _) = snapshot(&st).await;
    let t = ws.item("ghi-acme-app-12").unwrap();
    assert!(t.labels.contains(&"help wanted".to_string()));
    assert_eq!(t.owner.name, "tobias-lang");
    assert!(t.project.as_ref().unwrap().closed);
    assert!(t.comments.last().unwrap().own);
}

#[tokio::test]
async fn nothing_goes_out_when_the_rules_say_no() {
    let g = projects_server().await;
    let st = state(project_sources(&g.uri(), TicketsFrom::Both), true);
    sync(&st).await;
    let before = g.received_requests().await.unwrap().len();
    // The issue has no such label to remove, the field value is malformed, the draft cannot be closed.
    for a in [
        json!({"SetLabel": {"item": "ghi-acme-app-12", "label": "nope", "on": false}}),
        json!({"SetProjectField": {"item": "ghi-acme-app-12", "field_id": "PVTF_est", "value": {"Number": "many"}}}),
        json!({"SetIssueClosed": {"item": "ghd-pvti-draft1", "closed": true}}),
    ] {
        let (status, _) = action(&st, a).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert_eq!(g.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn a_new_issue_is_created_at_github_and_the_answer_is_booked_not_the_browsers() {
    let g = projects_server().await;
    rest(
        &g,
        "POST",
        "/repos/acme/app/issues",
        201,
        r#"{"number":99,"node_id":"I_99","html_url":"https://github.com/acme/app/issues/99"}"#,
    )
    .await;
    graphql(
        &g,
        "addProjectV2ItemById",
        r#"{"data":{"addProjectV2ItemById":{"item":{"id":"PVTI_99"}}}}"#,
    )
    .await;
    graphql(
        &g,
        "updateProjectV2ItemFieldValue",
        r#"{"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"PVTI_99"}}}}"#,
    )
    .await;
    let st = state(project_sources(&g.uri(), TicketsFrom::Both), true);
    sync(&st).await;
    let (status, body) = action(
        &st,
        json!({"CreateIssue": {"project": "PVT_acme3", "repo": "acme/app", "title": "Neu", "body": "Text",
            "status": "InProgress",
            // A made-up answer from the browser is dropped.
            "created": {"repo": "evil/x", "number": 1, "content_id": "I_evil", "item_id": "PVTI_evil", "url": ""}}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (ws, _) = snapshot(&st).await;
    assert!(ws.item_by_key("acme/app#99").is_some());
    assert!(ws.item_by_key("evil/x#1").is_none());
    let set = graphql_calls(&g, "updateProjectV2ItemFieldValue").await;
    assert_eq!(
        set[0]["variables"]["value"],
        json!({"singleSelectOptionId": "s_prog"})
    );
}

#[tokio::test]
async fn a_draft_is_converted_at_github_and_becomes_the_issue() {
    let g = projects_server().await;
    graphql(
        &g,
        "repository(owner",
        r#"{"data":{"repository":{"id":"R_app","nameWithOwner":"acme/app"}}}"#,
    )
    .await;
    graphql(&g, "convertProjectV2DraftIssueItemToIssue", r#"{"data":{"convertProjectV2DraftIssueItemToIssue":{"item":{"id":"PVTI_draft1","content":{"id":"I_77","number":77,"url":"https://github.com/acme/app/issues/77","repository":{"nameWithOwner":"acme/app"}}}}}}"#).await;
    let st = state(project_sources(&g.uri(), TicketsFrom::Both), true);
    sync(&st).await;
    let (status, body) = action(
        &st,
        json!({"ConvertDraft": {"item": "ghd-pvti-draft1", "repo": "acme/app"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (ws, _) = snapshot(&st).await;
    assert!(ws.item("ghd-pvti-draft1").is_none());
    assert_eq!(ws.item("ghi-acme-app-77").unwrap().key, "acme/app#77");
}

#[tokio::test]
async fn without_the_project_connection_nothing_is_written_or_invented() {
    let g = projects_server().await;
    let st = state(sources(&g.uri()), true);
    let (status, body) = action(
        &st,
        json!({"CreateIssue": {"project": "PVT_acme3", "repo": "acme/app", "title": "Neu"}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(g.received_requests().await.unwrap().is_empty());
}

async fn post_json(st: &AppState, uri: &str, body: serde_json::Value) -> (StatusCode, String) {
    let req = request("POST", uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send_request(st, req).await
}

#[tokio::test]
async fn a_project_is_created_on_click_for_the_owner_and_linked_to_the_repo() {
    let g = MockServer::start().await;
    get(&g, "/user", ok(json!({"login": "mkeller"}))).await;
    graphql(
        &g,
        "repositoryOwner(login: $login) { id login }",
        r#"{"data":{"repositoryOwner":{"id":"O_acme","login":"acme"}}}"#,
    )
    .await;
    graphql(&g, "createProjectV2(", r#"{"data":{"createProjectV2":{"projectV2":{"id":"PVT_new","number":7,"title":"Board","url":"https://github.com/orgs/acme/projects/7","closed":false,"owner":{"__typename":"Organization","login":"acme"}}}}}"#).await;
    graphql(
        &g,
        "repository(owner",
        r#"{"data":{"repository":{"id":"R_app","nameWithOwner":"acme/app"}}}"#,
    )
    .await;
    graphql(
        &g,
        "linkProjectV2ToRepository",
        r#"{"data":{"linkProjectV2ToRepository":{"repository":{"id":"R_app"}}}}"#,
    )
    .await;
    graphql(&g, "repositoryOwner(login: $login) { ... on ProjectV2Owner", r#"{"data":{"repositoryOwner":{"projectV2":{"id":"PVT_new","number":7,"title":"Board","url":"u","closed":false,"fields":{"nodes":[]}}}}}"#).await;
    graphql(&g, "createProjectV2Field", r#"{"data":{"createProjectV2Field":{"projectV2Field":{"id":"F","name":"Status","options":[]}}}}"#).await;
    let st = state(project_sources(&g.uri(), TicketsFrom::GitHubProjects), true);
    // Looking at the setup creates nothing.
    assert!(graphql_calls(&g, "createProjectV2(").await.is_empty());
    let (status, body) = post_json(
        &st,
        "/api/github/projects/create",
        json!({"owner": "acme", "title": "Board", "repo": "acme/app"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["project"]["number"], 7);
    assert_eq!(v["project"]["repos"], json!(["acme/app"]));
    assert!(!body.contains(TOKEN));
    for needle in [
        "createProjectV2(",
        "linkProjectV2ToRepository",
        "createProjectV2Field",
    ] {
        assert_eq!(graphql_calls(&g, needle).await.len(), 1, "{needle}");
    }
}

#[tokio::test]
async fn creating_a_project_checks_its_input_and_explains_a_missing_right() {
    let g = MockServer::start().await;
    graphql(
        &g,
        "repositoryOwner(login: $login) { id login }",
        r#"{"data":{"repositoryOwner":{"id":"O_acme","login":"acme"}}}"#,
    )
    .await;
    graphql(&g, "createProjectV2(", fixture!("gh_err_forbidden.json")).await;
    let st = state(project_sources(&g.uri(), TicketsFrom::Both), true);
    for bad in [
        json!({"owner": "a c", "title": "x"}),
        json!({"owner": "acme", "title": "  "}),
        json!({"owner": "acme", "title": "x", "repo": "not a repo"}),
    ] {
        let (status, _) = post_json(&st, "/api/github/projects/create", bad).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert!(
        g.received_requests().await.unwrap().is_empty(),
        "bad input never reaches GitHub"
    );
    let (status, body) = post_json(
        &st,
        "/api/github/projects/create",
        json!({"owner": "acme", "title": "Board"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        crate::german::german(&body).contains("`project`")
            && crate::german::german(&body).contains("GitHub Projects ändern"),
        "{body}"
    );
    // Without a token: a plain message, not a call.
    let none = state(project_sources(&g.uri(), TicketsFrom::Both), false);
    let before = g.received_requests().await.unwrap().len();
    let (status, _) = post_json(
        &none,
        "/api/github/projects/create",
        json!({"owner": "acme", "title": "Board"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(g.received_requests().await.unwrap().len(), before);
}
