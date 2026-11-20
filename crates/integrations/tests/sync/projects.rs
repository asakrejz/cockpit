//! GitHub Projects as a source of tickets: GraphQL against replayed answers (fixtures written from
//! the documented schema, never recorded from the real service).

use cockpit_core::{
    Access, Kind, ProjectRef, Right, Scope, TicketStatus, TokenKind, project::ProjectOrigin,
};
use cockpit_integrations::ProjectsService;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path};

use super::*;

fn proj(owner: &str, n: u32) -> ProjectRef {
    ProjectRef::new(owner, n)
}

fn client(s: &MockServer, token: &str) -> GitHubClient {
    GitHubClient::new(&s.uri(), Secret::new(token)).unwrap()
}

fn svc(s: &MockServer) -> Services {
    Services {
        projects: Some(ProjectsService {
            client: client(s, "ghp_x"),
            projects: vec![proj("acme", 3)],
            status_field: None,
            team_field: None,
        }),
        ..Default::default()
    }
}

/// A POST to `/graphql` whose body contains `needle` answers `text`.
async fn gql(s: &MockServer, needle: &str, text: &str) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(needle))
        .respond_with(ok(text))
        .mount(s)
        .await;
}

/// The same, but only when the body also holds `after` (the cursor of the page).
async fn gql_page(s: &MockServer, cursor: Option<&str>, text: &str) {
    let m = Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("ProjectV2 {"));
    let m = match cursor {
        Some(c) => m.and(body_string_contains(format!("\"after\":\"{c}\""))),
        None => m.and(body_string_contains("\"after\":null")),
    };
    m.respond_with(ok(text)).mount(s).await;
}

async fn project_server() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(&s, "repositoryOwner", fixture!("gh_project_meta.json")).await;
    gql_page(&s, None, fixture!("gh_project_items_1.json")).await;
    gql_page(&s, Some("c1"), fixture!("gh_project_items_2.json")).await;
    gql(&s, "timelineItems", fixture!("gh_issue_details.json")).await;
    s
}

async fn count_calls(s: &MockServer, needle: &str) -> usize {
    s.received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/graphql" && String::from_utf8_lossy(&r.body).contains(needle))
        .count()
}

fn only(remote: &Remote) -> &cockpit_core::Loaded {
    match &remote.sources[0].outcome {
        Outcome::Loaded(l) => l,
        Outcome::Failed(p) => panic!("source failed: {p:?}"),
    }
}

fn problem(remote: &Remote) -> &Problem {
    match &remote.sources[0].outcome {
        Outcome::Failed(p) => p,
        Outcome::Loaded(_) => panic!("source did not fail"),
    }
}

// ---------- The list of projects ----------

#[tokio::test]
async fn the_list_has_user_org_and_repo_projects_once_each() {
    let s = MockServer::start().await;
    gql(
        &s,
        "organizations(first",
        fixture!("gh_projects_viewer.json"),
    )
    .await;
    gql(&s, "r0: repository", fixture!("gh_projects_repos.json")).await;
    let list = client(&s, "ghp_x")
        .list_projects(&["acme/app".into(), "acme/secret".into(), "not a repo".into()])
        .await
        .unwrap();
    assert_eq!(list.owners, ["mkeller", "acme"]);
    let list = list.projects;
    let names: Vec<_> = list
        .iter()
        .map(|p| format!("{}/{}", p.owner, p.number))
        .collect();
    // Sorted by owner and number; the project that both the org and a repo list is one entry.
    assert_eq!(names, ["acme/2", "acme/3", "acme/9", "mkeller/1"]);
    let acme3 = list.iter().find(|p| p.number == 3).unwrap();
    assert_eq!(acme3.origin, ProjectOrigin::Org);
    assert_eq!(acme3.repos, ["acme/app"]);
    assert!(list.iter().find(|p| p.number == 2).unwrap().closed);
    assert_eq!(
        list.iter().find(|p| p.number == 9).unwrap().origin,
        ProjectOrigin::Repo
    );
    assert_eq!(
        list.iter().find(|p| p.owner == "mkeller").unwrap().origin,
        ProjectOrigin::User
    );
    // An invalid repo name never reaches the query; the hidden repo is no error.
    assert_eq!(count_calls(&s, "r0: repository").await, 1);
    assert_eq!(count_calls(&s, "r2: repository").await, 0);
}

#[tokio::test]
async fn the_project_meta_has_the_fields_and_their_choices() {
    let s = project_server().await;
    let meta = client(&s, "ghp_x")
        .project_meta(&proj("acme", 3), None)
        .await
        .unwrap();
    assert_eq!(
        (meta.id.as_str(), meta.title.as_str()),
        ("PVT_acme3", "App roadmap")
    );
    let status = meta.status_def().unwrap();
    assert_eq!(status.options.len(), 4);
    let sprint = meta.field_named("sprint").unwrap();
    // Running and finished iterations are both choices.
    assert_eq!(sprint.options.len(), 2);
    assert_eq!(
        meta.field_named("estimate").unwrap().kind,
        cockpit_core::FieldKind::Number
    );
}

#[tokio::test]
async fn a_project_that_is_not_visible_is_not_found() {
    let s = MockServer::start().await;
    gql(
        &s,
        "repositoryOwner",
        r#"{"data":{"repositoryOwner":{"projectV2":null}},"errors":[{"type":"NOT_FOUND","path":["repositoryOwner","projectV2"],"message":"Could not resolve to a ProjectV2 with the number 99."}]}"#,
    )
    .await;
    let err = client(&s, "ghp_x")
        .project_meta(&proj("acme", 99), None)
        .await
        .unwrap_err();
    assert_eq!(err, Error::NotFound);
}

use cockpit_integrations::Error;

// ---------- Items as tickets ----------

#[tokio::test]
async fn items_arrive_as_tickets_across_two_pages() {
    let s = project_server().await;
    let (ws, remote) = sync(&svc(&s), &Workspace::default(), NOW_MS).await;
    let loaded = only(&remote);
    // Two issues on the first page, one draft, one on the second; the pull request and the
    // archived item are left out.
    assert_eq!(loaded.items.len(), 3);
    assert_eq!(count_calls(&s, "ProjectV2 {").await, 2, "one call per page");
    assert!(
        loaded
            .items
            .iter()
            .all(|i| i.source == Source::GitHubProject && i.kind == Kind::Ticket)
    );

    let t = ws.item_by_key("acme/app#12").expect("issue 12");
    assert_eq!(t.title, "Fix the avatar upload");
    assert_eq!(t.status, TicketStatus::InProgress);
    assert_eq!(t.scope, Scope::Mine, "assigned to me");
    assert_eq!(t.owner, cockpit_core::Person::me());
    assert_eq!(t.co_assignees, ["tobias-lang"]);
    assert_eq!(t.labels, ["bug", "upload"]);
    assert_eq!(t.milestone.as_deref(), Some("v1.4"));
    assert_eq!(t.reporter.as_deref(), Some("tobias-lang"));
    assert_eq!(t.repo.as_deref(), Some("acme/app"));
    assert_eq!(t.priority, cockpit_core::Priority::High);
    assert_eq!(t.team.as_ref().map(|x| x.name.as_str()), Some("Core"));
    assert_eq!(t.due.as_deref(), Some("2024-05-20"));
    assert_eq!(
        t.url.as_deref(),
        Some("https://github.com/acme/app/issues/12")
    );
    // The body stays Markdown; it is not turned into Jira formatting.
    assert!(t.description.contains("**upload**") && t.description_rich.is_none());
    // Sprint 21 runs on 2024-05-14.
    let sprint = t.sprint.as_ref().unwrap();
    assert_eq!(sprint.name, "Sprint 21");
    assert_eq!(sprint.state, cockpit_core::SprintState::Active);
    assert_eq!(sprint.end, "2024-05-27");
    // Project fields beyond the status are kept as values.
    let p = t.project.as_ref().unwrap();
    assert_eq!(p.project, "acme/3");
    assert_eq!(p.item_id, "PVTI_12");
    assert_eq!(p.content_id, "I_12");
    assert_eq!(p.status_option.as_deref(), Some("s_prog"));
    assert!(
        p.fields
            .iter()
            .any(|f| f.name == "Estimate" && f.text == "5")
    );
    assert!(
        p.fields
            .iter()
            .any(|f| f.name == "Notes" && f.text == "Check with design")
    );
    assert!(
        p.fields
            .iter()
            .all(|f| f.name != "Status" && f.name != "Title")
    );

    // The closed issue without an assignee is done and belongs to the team.
    let done = ws.item_by_key("acme/web#15").unwrap();
    assert_eq!(done.status, TicketStatus::Done);
    assert_eq!(done.scope, Scope::Team);
    assert_eq!(done.owner.name, cockpit_core::UNASSIGNED_NAME);
    assert_eq!(done.repo.as_deref(), Some("acme/web"));
}

#[tokio::test]
async fn a_draft_is_a_ticket_without_an_issue_number() {
    let s = project_server().await;
    let (ws, _) = sync(&svc(&s), &Workspace::default(), NOW_MS).await;
    let draft = ws
        .items
        .iter()
        .find(|i| i.title == "Think about dark mode")
        .unwrap();
    let p = draft.project.as_ref().unwrap();
    assert!(p.draft && p.number.is_none() && p.content_id.is_empty());
    assert!(draft.key.starts_with("acme/3#draft-"), "{}", draft.key);
    assert_eq!(draft.repo, None);
    assert_eq!(draft.status, TicketStatus::Open);
    assert_eq!(draft.reporter.as_deref(), Some("Du"));
}

#[tokio::test]
async fn comments_linked_prs_parent_and_type_come_with_the_details() {
    let s = project_server().await;
    let (ws, _) = sync(&svc(&s), &Workspace::default(), NOW_MS).await;
    let t = ws.item_by_key("acme/app#12").unwrap();
    assert_eq!(t.comments.len(), 2);
    assert_eq!(t.comments[0].text, "Reproduced with `avatar.png`.");
    assert!(!t.comments[0].own && t.comments[1].own);
    let p = t.project.as_ref().unwrap();
    // The connected PR and the closing reference are one; the cross-reference is another.
    let prs: Vec<_> = p.linked_prs.iter().map(|k| k.to_string()).collect();
    assert_eq!(prs, ["acme/app#20", "acme/web#4"]);
    assert_eq!(t.issue_type.as_deref(), Some("Bug"));
    assert_eq!(p.parent.as_ref().unwrap().key, "acme/app#10");
    assert_eq!(p.sub_issues.len(), 2);
    assert!(p.sub_issues[0].done && !p.sub_issues[1].done);
    let kinds: Vec<_> = t
        .details
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .map(|f| f.id.as_str())
        .collect();
    assert!(
        kinds.contains(&"issuetype")
            && kinds.contains(&"parent")
            && kinds.contains(&"ghp:sub_issues")
    );
}

#[tokio::test]
async fn a_server_without_issue_types_still_syncs() {
    let s = project_server().await;
    // The first details call (with the extras) is rejected, the one without them works.
    s.reset().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(&s, "repositoryOwner", fixture!("gh_project_meta.json")).await;
    gql_page(&s, None, fixture!("gh_project_items_1.json")).await;
    gql_page(&s, Some("c1"), fixture!("gh_project_items_2.json")).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("issueType"))
        .respond_with(ok(r#"{"data":null,"errors":[{"path":["query","nodes","... on Issue","issueType"],"extensions":{"code":"undefinedField"},"message":"Field 'issueType' doesn't exist on type 'Issue'"}]}"#))
        .mount(&s)
        .await;
    gql(&s, "timelineItems", fixture!("gh_issue_details.json")).await;
    let (ws, remote) = sync(&svc(&s), &Workspace::default(), NOW_MS).await;
    let t = ws.item_by_key("acme/app#12").unwrap();
    assert_eq!(t.comments.len(), 2);
    assert_eq!(
        only(&remote).warnings.len(),
        1,
        "one hint about the missing extras"
    );
}

// ---------- Incremental ----------

#[tokio::test]
async fn an_unchanged_issue_is_not_asked_for_its_details_again() {
    let s = project_server().await;
    let d = svc(&s);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(count_calls(&s, "timelineItems").await, 1);
    // Second sync: nothing changed, so the heavy query does not run again – and the comments stay.
    let (ws2, _) = sync(&d, &ws, NOW_MS + 60_000).await;
    assert_eq!(count_calls(&s, "timelineItems").await, 1);
    assert_eq!(ws2.item_by_key("acme/app#12").unwrap().comments.len(), 2);
    assert_eq!(ws2.items.len(), ws.items.len());
}

#[tokio::test]
async fn only_the_issue_that_moved_gets_its_details_again() {
    let s = project_server().await;
    let d = svc(&s);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    // Issue 15 changes at GitHub.
    s.reset().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(&s, "repositoryOwner", fixture!("gh_project_meta.json")).await;
    gql_page(&s, None, fixture!("gh_project_items_1.json")).await;
    gql_page(
        &s,
        Some("c1"),
        &fixture!("gh_project_items_2.json")
            .replace("2024-05-13T12:00:00Z", "2024-05-14T11:59:00Z"),
    )
    .await;
    gql(&s, "timelineItems", fixture!("gh_issue_details.json")).await;
    let (_, _) = sync(&d, &ws, NOW_MS + 60_000).await;
    let calls: Vec<Value> = s
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| String::from_utf8_lossy(&r.body).contains("timelineItems"))
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["variables"]["ids"], json!(["I_15"]));
}

#[tokio::test]
async fn an_issue_that_left_the_project_disappears() {
    let s = project_server().await;
    let d = svc(&s);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert!(ws.item_by_key("acme/web#15").is_some());
    s.reset().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(&s, "repositoryOwner", fixture!("gh_project_meta.json")).await;
    gql_page(
        &s,
        None,
        &fixture!("gh_project_items_1.json")
            .replace("\"hasNextPage\":true", "\"hasNextPage\":false"),
    )
    .await;
    gql(&s, "timelineItems", fixture!("gh_issue_details.json")).await;
    let (ws2, _) = sync(&d, &ws, NOW_MS + 60_000).await;
    assert!(ws2.item_by_key("acme/web#15").is_none());
    assert!(ws2.item_by_key("acme/app#12").is_some());
}

// ---------- Rate limit ----------

#[tokio::test]
async fn an_empty_budget_stops_the_sync_before_the_next_call_and_keeps_the_data() {
    let s = project_server().await;
    let d = svc(&s);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let before = ws.items.len();
    // The first page says only 3 points are left until far in the future.
    s.reset().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(
        &s,
        "repositoryOwner",
        &fixture!("gh_project_meta.json").replace(
            "\"remaining\":4980,\"resetAt\":\"2024-05-14T13:00:00Z\"",
            "\"remaining\":3,\"resetAt\":\"2999-01-01T00:00:00Z\"",
        ),
    )
    .await;
    gql_page(&s, None, fixture!("gh_project_items_1.json")).await;
    let (ws2, remote) = sync(&d, &ws, NOW_MS + 60_000).await;
    assert!(matches!(
        problem(&remote),
        Problem::RateLimited { until_ms: Some(_) }
    ));
    assert_eq!(
        count_calls(&s, "ProjectV2 {").await,
        0,
        "no call after the budget ran out"
    );
    assert_eq!(ws2.items.len(), before, "the data stays");
    let state = ws2.sync.state_of(Source::GitHubProject).unwrap();
    assert_eq!(state.id(), "quota");
}

#[tokio::test]
async fn a_rate_limited_answer_pauses_the_source() {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(&s, "repositoryOwner", fixture!("gh_err_rate_limited.json")).await;
    let (_, remote) = sync(&svc(&s), &Workspace::default(), NOW_MS).await;
    assert!(matches!(problem(&remote), Problem::RateLimited { .. }));
}

// ---------- Missing rights ----------

#[tokio::test]
async fn a_classic_token_without_read_project_is_told_the_scope() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ok(fixture!("github_user.json")).insert_header("x-oauth-scopes", "repo"))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(
            ok(fixture!("gh_err_insufficient_scopes.json")).insert_header("x-oauth-scopes", "repo"),
        )
        .mount(&s)
        .await;
    let (ws, remote) = sync(&svc(&s), &Workspace::default(), NOW_MS).await;
    assert_eq!(
        problem(&remote),
        &Problem::MissingRight {
            right: Right::ProjectsRead,
            kind: TokenKind::Classic
        }
    );
    let text = problem(&remote).msg().render(cockpit_i18n::Lang::En);
    assert!(text.contains("`read:project`"), "{text}");
    // The setup banner says it, too.
    let notice = cockpit_core::source_notices(&ws.sync, NOW_MS);
    assert_eq!(notice.len(), 1);
    assert!(
        notice[0]
            .detail
            .render(cockpit_i18n::Lang::En)
            .contains("read:project")
    );
}

#[tokio::test]
async fn a_fine_grained_token_is_told_the_permission_by_the_refusal() {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(&s, "repositoryOwner", fixture!("gh_err_forbidden.json")).await;
    let d = Services {
        projects: Some(ProjectsService {
            client: client(&s, "github_pat_11AAA"),
            projects: vec![proj("acme", 3)],
            status_field: None,
            team_field: None,
        }),
        ..Default::default()
    };
    let (_, remote) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(
        problem(&remote),
        &Problem::MissingRight {
            right: Right::ProjectsRead,
            kind: TokenKind::FineGrained
        }
    );
    let text = problem(&remote).msg().render(cockpit_i18n::Lang::En);
    assert!(
        text.contains("\"Projects\"") && text.contains("fine-grained"),
        "{text}"
    );
}

#[tokio::test]
async fn a_refusal_below_content_is_about_issues_not_projects() {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(&s, "repositoryOwner", fixture!("gh_project_meta.json")).await;
    gql(&s, "ProjectV2 {", fixture!("gh_err_forbidden_content.json")).await;
    let d = Services {
        projects: Some(ProjectsService {
            client: client(&s, "ghu_app").with_app(true),
            projects: vec![proj("acme", 3)],
            status_field: None,
            team_field: None,
        }),
        ..Default::default()
    };
    let (_, remote) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(
        problem(&remote),
        &Problem::MissingRight {
            right: Right::IssuesRead,
            kind: TokenKind::App
        }
    );
    let text = problem(&remote).msg().render(cockpit_i18n::Lang::De);
    assert!(
        text.contains("GitHub App") && text.contains("Issues"),
        "{text}"
    );
}

#[tokio::test]
async fn the_probe_reads_the_scopes_of_a_classic_token() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(
            ok(fixture!("github_user.json")).insert_header("x-oauth-scopes", "repo, read:project"),
        )
        .mount(&s)
        .await;
    let c = client(&s, "ghp_x");
    let (login, access) = c.probe_access().await.unwrap();
    assert_eq!(login, "mkeller");
    assert_eq!(access.kind, TokenKind::Classic);
    assert_eq!(
        access.scopes.as_deref(),
        Some(&["repo".to_string(), "read:project".to_string()][..])
    );
    assert_eq!(
        access.missing(&[
            Right::ProjectsRead,
            Right::ProjectsWrite,
            Right::IssuesWrite
        ]),
        [Right::ProjectsWrite]
    );
    // A fine-grained token gets no header: the kind comes from its shape, the scopes stay unknown.
    let s2 = MockServer::start().await;
    get(&s2, "/user", ok(fixture!("github_user.json"))).await;
    let (_, a2) = client(&s2, "github_pat_x").probe_access().await.unwrap();
    assert_eq!(
        a2,
        Access {
            kind: TokenKind::FineGrained,
            scopes: None
        }
    );
}

// ---------- Hints and small things ----------

#[tokio::test]
async fn a_project_without_a_status_field_says_so_and_counts_everything_as_open() {
    let s = project_server().await;
    let d = Services {
        projects: Some(ProjectsService {
            client: client(&s, "ghp_x"),
            projects: vec![proj("acme", 3)],
            status_field: Some("Stage".into()),
            team_field: Some("Squad".into()),
        }),
        ..Default::default()
    };
    let (ws, remote) = sync(&d, &Workspace::default(), NOW_MS).await;
    assert_eq!(only(&remote).warnings.len(), 1);
    let t = ws.item_by_key("acme/app#12").unwrap();
    assert_eq!(t.status, TicketStatus::Open);
    // The team field is the one named in the setup.
    assert!(t.team.is_none());
    // A closed issue without a status column is done.
    assert_eq!(
        ws.item_by_key("acme/web#15").unwrap().status,
        TicketStatus::Done
    );
}

#[tokio::test]
async fn a_hidden_project_is_a_hint_and_its_old_tickets_stay() {
    let s = project_server().await;
    let d = svc(&s);
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    s.reset().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(
        &s,
        "repositoryOwner",
        r#"{"data":{"repositoryOwner":{"projectV2":null}},"errors":[{"type":"NOT_FOUND","path":["repositoryOwner","projectV2"],"message":"Could not resolve"}]}"#,
    )
    .await;
    let (ws2, remote) = sync(&d, &ws, NOW_MS + 60_000).await;
    assert_eq!(only(&remote).warnings.len(), 1);
    assert_eq!(
        ws2.items.len(),
        ws.items.len(),
        "nothing vanishes for one bad minute"
    );
}

#[test]
fn the_graphql_address_fits_github_and_enterprise_servers() {
    let c = GitHubClient::new("http://127.0.0.1:9/api/v3", Secret::new("x")).unwrap();
    assert_eq!(c.graphql_url(), "http://127.0.0.1:9/api/graphql");
    let c = GitHubClient::new("http://127.0.0.1:9", Secret::new("x")).unwrap();
    assert_eq!(c.graphql_url(), "http://127.0.0.1:9/graphql");
}

#[tokio::test]
async fn nothing_is_sent_to_the_project_source_when_it_is_paused() {
    let s = project_server().await;
    let remote = svc(&s)
        .fetch(&Workspace::default(), NOW_MS, &[Source::GitHubProject])
        .await;
    assert!(remote.sources.is_empty());
    assert!(s.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_assignees_of_a_repo_are_searched_by_login() {
    let s = MockServer::start().await;
    get(
        &s,
        "/repos/acme/app/assignees",
        ok(r#"[{"login":"mkeller"},{"login":"tobias-lang"},{"login":"Mira-T"}]"#),
    )
    .await;
    let c = client(&s, "ghp_x");
    assert_eq!(c.assignees("acme/app", "mi").await.unwrap(), ["Mira-T"]);
    assert_eq!(c.assignees("acme/app", "").await.unwrap().len(), 3);
    assert_eq!(
        c.assignees("acme/app", " LANG ").await.unwrap(),
        ["tobias-lang"]
    );
    // Nothing is sent for something that is no repo.
    assert!(c.assignees("../etc", "x").await.is_err());
}
