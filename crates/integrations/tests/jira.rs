//! Jira: Abbildung und Client gegen nachgebaute Antworten (`tests/fixtures`).

use cockpit_core::{Kind, ReasonKind, Scope, Source, TicketStatus};
use cockpit_integrations::jira::{
    ASSIGNED_OR_WATCHED_JQL, JiraClient, comments_to_core, dto::*, issue_to_item, jql_for_project,
};
use cockpit_integrations::{Error, Secret, time};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("fixtures/", $name))
    };
}

/// 2024-05-14T12:00:00Z
fn now() -> i64 {
    time::parse_iso("2024-05-14T12:00:00Z").unwrap()
}

fn search() -> SearchResult {
    serde_json::from_str(fixture!("jira_search.json")).unwrap()
}

#[test]
fn assigned_issue() {
    let s = search();
    let item = issue_to_item(&s.issues[0], "acc-mira", now());
    assert_eq!(item.id, "bau-142");
    assert_eq!(item.key, "BAU-142");
    assert_eq!((item.kind, item.source), (Kind::Ticket, Source::Jira));
    assert_eq!(item.title, "Avatar-Größe über Tokens steuern");
    assert_eq!(
        item.description,
        "Feste Pixelwerte ersetzen.\n• Light und Dark prüfen"
    );
    assert_eq!(item.status, TicketStatus::InProgress);
    assert_eq!(
        (item.reason, item.scope),
        (ReasonKind::Assigned, Scope::Mine)
    );
    assert_eq!(item.owner, cockpit_core::Person::me());
    // 09:12 +02:00 = 07:12 UTC → 4 h 48 min vor 12:00 UTC
    assert_eq!(item.time, "vor 4 Std.");
    assert!(item.pr.is_none() && item.branch.is_none() && item.comments.is_empty());
    assert_eq!(item.labels, ["a11y", "tokens"]);
    assert_eq!(item.repo, None, "Tickets haben kein Repo");
    // Ohne Feld `labels` (Issue ohne Labels) bleibt die Liste leer.
    assert!(
        issue_to_item(&s.issues[1], "acc-mira", now())
            .labels
            .is_empty()
    );
}

#[test]
fn watched_issue_in_review() {
    let s = search();
    let item = issue_to_item(&s.issues[1], "acc-mira", now());
    assert_eq!(item.status, TicketStatus::Review);
    assert_eq!(
        (item.reason, item.scope),
        (ReasonKind::Comment, Scope::Team)
    );
    assert_eq!(item.owner.name, "Tobias Lang");
    assert_eq!(item.owner.initials, "TL");
    assert_eq!(item.description, "");
    assert_eq!(item.time, "vor 4 Tagen");
}

#[test]
fn issue_without_assignee_and_open() {
    let s = search();
    let item = issue_to_item(&s.issues[2], "acc-mira", now());
    assert_eq!(item.status, TicketStatus::Open);
    // Ohne Bearbeiter gilt der Melder als Besitzer.
    assert_eq!(item.owner.name, "Tobias Lang");

    let mut without_all = s.issues[2].clone();
    without_all.fields.reporter = None;
    assert_eq!(
        issue_to_item(&without_all, "acc-mira", now()).owner.name,
        "Nicht zugewiesen"
    );
}

#[test]
fn done_is_detected() {
    let mut issue = search().issues.remove(0);
    issue.fields.status.status_category = Some(StatusCategory { key: "done".into() });
    assert_eq!(
        issue_to_item(&issue, "x", now()).status,
        TicketStatus::Done
    );
    issue.fields.status.status_category = None;
    assert_eq!(
        issue_to_item(&issue, "x", now()).status,
        TicketStatus::Open
    );
}

#[test]
fn comments_with_adf_and_own() {
    let page: CommentPage = serde_json::from_str(fixture!("jira_comments_142.json")).unwrap();
    let c = comments_to_core(&page.comments, "acc-mira", now());
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].text, "@Du kannst du das übernehmen?");
    assert_eq!(c[0].author.name, "Tobias Lang");
    assert!(!c[0].own);
    assert_eq!(c[1].text, "Ja, bin dran.");
    assert!(c[1].own);
    assert_eq!(c[1].time, "vor 4 Std.");
}

// ---------- Client gegen Mock-Server ----------

fn response(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "application/json")
}

#[tokio::test]
async fn inbox_load_with_jql_and_sign_in() {
    let server = MockServer::start().await;
    // base64("mira@firma.example:jira-token")
    let basic = "Basic bWlyYUBmaXJtYS5leGFtcGxlOmppcmEtdG9rZW4=";
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .and(header("authorization", basic))
        .respond_with(response(fixture!("jira_myself.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("jql", ASSIGNED_OR_WATCHED_JQL))
        .and(header("authorization", basic))
        .respond_with(response(fixture!("jira_search.json")))
        .mount(&server)
        .await;

    let c = JiraClient::new(
        &server.uri(),
        "mira@firma.example",
        Secret::new("jira-token"),
    )
    .unwrap();
    let items = c.load_inbox(now()).await.unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].scope, Scope::Mine);
    assert_eq!(items[1].scope, Scope::Team);
}

#[tokio::test]
async fn comments_and_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/BAU-142/comment"))
        .respond_with(response(fixture!("jira_comments_142.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/BAU-1/comment"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let c = JiraClient::new(&server.uri(), "a@b.de", Secret::new("t")).unwrap();
    assert_eq!(c.comments("BAU-142").await.unwrap().len(), 2);
    assert_eq!(
        c.comments("BAU-1").await.unwrap_err(),
        Error::NotFound
    );
    assert_eq!(c.me().await.unwrap_err(), Error::NotSignedIn);
}

#[tokio::test]
async fn projects_page_by_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/project/search"))
        .and(query_param("startAt", "0"))
        .and(query_param("orderBy", "name"))
        .respond_with(response(fixture!("jira_projects_1.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/project/search"))
        .and(query_param("startAt", "2"))
        .respond_with(response(fixture!("jira_projects_2.json")))
        .expect(1)
        .mount(&server)
        .await;

    let c = JiraClient::new(&server.uri(), "a@b.de", Secret::new("t")).unwrap();
    let projects = c.projects().await.unwrap();
    let keys: Vec<_> = projects.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["BAU", "OPS", "WEB"]);
    assert_eq!(projects[0].name, "Baukasten");
}

#[test]
fn query_for_a_project() {
    assert_eq!(
        jql_for_project("BAU").unwrap(),
        "project = BAU AND statusCategory != Done ORDER BY updated DESC"
    );
    for broken in ["", "1AB", "A) OR (B", "BAU-1"] {
        assert!(
            matches!(jql_for_project(broken), Err(Error::Config(_))),
            "{broken}"
        );
    }
}

#[test]
fn debug_does_not_show_the_key() {
    let c = JiraClient::new(
        "https://firma.atlassian.net",
        "a@b.de",
        Secret::new("geheim-123"),
    )
    .unwrap();
    assert!(!format!("{c:?}").contains("geheim-123"));
}
