//! Jira: mapping and client against replayed responses (`tests/fixtures`).

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

/// A timestamp of a fixture in milliseconds.
fn ms(iso: &str) -> Option<u64> {
    time::parse_iso(iso).map(|s| s as u64 * 1000)
}

fn search() -> SearchResult {
    serde_json::from_str(fixture!("jira_search.json")).unwrap()
}

#[test]
fn assigned_issue() {
    let s = search();
    let item = issue_to_item(&s.issues[0], "acc-mira");
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
    // The time is a timestamp the interface renders ("4 hours ago"), not a text.
    assert_eq!(item.updated_ms, ms("2024-05-14T09:12:33.456+0200"));
    assert!(item.time.is_empty() && item.reason_text.is_empty());
    assert_eq!(item.reason_line().de(), "Dir zugewiesen");
    assert_eq!(
        item.reason_line().render(cockpit_i18n::Lang::En),
        "Assigned to you"
    );
    assert!(item.pr.is_none() && item.branch.is_none() && item.comments.is_empty());
    assert_eq!(item.labels, ["a11y", "tokens"]);
    assert_eq!(item.repo, None, "tickets have no repo");
    // Without the field `labels` (issue without labels) the list stays empty.
    assert!(issue_to_item(&s.issues[1], "acc-mira").labels.is_empty());
}

#[test]
fn watched_issue_in_review() {
    let s = search();
    let item = issue_to_item(&s.issues[1], "acc-mira");
    assert_eq!(item.status, TicketStatus::Review);
    assert_eq!(
        (item.reason, item.scope),
        (ReasonKind::Comment, Scope::Team)
    );
    assert_eq!(item.owner.name, "Tobias Lang");
    assert_eq!(item.owner.initials, "TL");
    assert_eq!(item.description, "");
    assert_eq!(item.updated_ms, ms("2024-05-10T08:00:00.000+0200"));
    assert_eq!(item.reason_line().de(), "Beobachtet");
}

#[test]
fn issue_without_assignee_and_open() {
    let s = search();
    let item = issue_to_item(&s.issues[2], "acc-mira");
    assert_eq!(item.status, TicketStatus::Open);
    // Without an assignee nobody owns the ticket, even if a reporter is known.
    assert_eq!(item.owner.name, "Nicht zugewiesen");
    assert_eq!(item.owner.initials, "–");
}

#[test]
fn done_is_detected() {
    let mut issue = search().issues.remove(0);
    issue.fields.status.status_category = Some(StatusCategory { key: "done".into() });
    assert_eq!(issue_to_item(&issue, "x").status, TicketStatus::Done);
    issue.fields.status.status_category = None;
    assert_eq!(issue_to_item(&issue, "x").status, TicketStatus::Open);
}

#[test]
fn comments_with_adf_and_own() {
    let page: CommentPage = serde_json::from_str(fixture!("jira_comments_142.json")).unwrap();
    let c = comments_to_core(&page.comments, "acc-mira");
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].text, "@Du kannst du das übernehmen?");
    assert_eq!(c[0].author.name, "Tobias Lang");
    assert!(!c[0].own);
    assert_eq!(c[1].text, "Ja, bin dran.");
    assert!(c[1].own);
    assert_eq!(c[1].created_ms, ms("2024-05-14T09:30:00.000+0200"));
    assert!(c[1].time.is_empty());
}

// ---------- Client against mock server ----------

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
    let items = c.load_inbox().await.unwrap();
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
    assert_eq!(c.comments("BAU-1").await.unwrap_err(), Error::NotFound);
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
        "project = BAU AND sprint in openSprints() ORDER BY updated DESC"
    );
    for broken in ["", "1AB", "A) OR (B", "BAU-1"] {
        assert!(
            matches!(
                jql_for_project(broken),
                Err(Error::Config(_) | Error::Coded(_))
            ),
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

#[test]
fn the_issue_type_is_read_for_the_branch_name() {
    let mut s = search();
    // Without the field (an older response) there is no type.
    assert_eq!(issue_to_item(&s.issues[0], "acc-mira").issue_type, None);
    s.issues[0].fields.issuetype = Some(IssueType { name: "Bug".into() });
    let item = issue_to_item(&s.issues[0], "acc-mira");
    assert_eq!(item.issue_type.as_deref(), Some("Bug"));
    let kind = cockpit_core::branch::BranchType::of(item.issue_type.as_deref(), &item.labels);
    assert_eq!(kind, cockpit_core::branch::BranchType::Fix);
}

#[tokio::test]
async fn searches_assignable_people_read_only() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .respond_with(response(fixture!("jira_myself.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/user/assignable/search"))
        .and(query_param("project", "BAU"))
        .and(query_param("query", "mi"))
        .respond_with(response(
            r#"[
              {"accountId":"acc-mira","displayName":"Mira Keller","active":true,"accountType":"atlassian"},
              {"accountId":"acc-timo","displayName":"Timo Böhm","active":true,"accountType":"atlassian"},
              {"accountId":"acc-gone","displayName":"Old Account","active":false,"accountType":"atlassian"},
              {"accountId":"acc-bot","displayName":"Some Bot","active":true,"accountType":"app"}
            ]"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let c = JiraClient::new(&server.uri(), "a@b.de", Secret::new("t")).unwrap();
    let people = c.assignable_people("BAU", " mi ").await.unwrap();
    let names: Vec<_> = people.iter().map(|p| p.name.as_str()).collect();
    // Mira is the key holder (see jira_myself.json): she comes as „you“.
    assert_eq!(
        names,
        [cockpit_core::Person::me().name.as_str(), "Timo Böhm"]
    );
    assert_eq!(people[1].initials, "TB");
    // Only GET requests went out.
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|r| r.method == wiremock::http::Method::GET)
    );
}

#[tokio::test]
async fn assignable_search_rejects_a_broken_project_key() {
    let server = MockServer::start().await;
    let c = JiraClient::new(&server.uri(), "a@b.de", Secret::new("t")).unwrap();
    assert!(c.assignable_people("A) OR (B", "x").await.is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
}
