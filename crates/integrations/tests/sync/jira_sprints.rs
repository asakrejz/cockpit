//! Jira: the active sprints by default, the sprint field of the site, the fallback for projects
//! without sprints, and errors that stay errors.

use super::*;
use cockpit_core::{Source as Src, SprintState};
use cockpit_i18n::Lang;
use cockpit_integrations::jira::{jql_for, open_tickets_jql, sprint_of};

const SPRINT_JQL: &str = "sprint in openSprints() AND project in (BAU) ORDER BY updated DESC";
const OPEN_JQL: &str = "statusCategory != Done AND project in (BAU) ORDER BY updated DESC";

fn issue(key: &str, sprint: serde_json::Value, category: &str) -> serde_json::Value {
    json!({"key": key, "fields": {
        "summary": format!("T {key}"),
        "status": {"name": "Status", "statusCategory": {"key": category}},
        "assignee": {"accountId": "x", "displayName": "Lena Koch"},
        "updated": "2024-05-14T10:00:00Z",
        "customfield_10020": sprint
    }})
}

fn sprint(name: &str, state: &str) -> serde_json::Value {
    json!({"id": 7, "name": name, "state": state,
        "startDate": "2024-05-06T08:00:00.000Z", "endDate": "2024-05-20T08:00:00.000Z"})
}

async fn site(fields: serde_json::Value) -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    get(&s, "/rest/api/3/field", ok(&fields.to_string())).await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/rest/api/3/issue/.*/comment$"))
        .respond_with(ok(r#"{"comments":[]}"#))
        .mount(&s)
        .await;
    s
}

fn sprint_fields() -> serde_json::Value {
    json!([
        {"id": "summary", "name": "Summary", "schema": {"type": "string"}},
        {"id": "customfield_10001", "name": "Sprint (alt)", "schema": {"type": "string"}},
        {"id": "customfield_10020", "name": "Sprint",
         "schema": {"type": "array", "custom": "com.pyxis.greenhopper.jira:gh-sprint"}}
    ])
}

async fn answer_jql(s: &MockServer, jql: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("jql", jql))
        .respond_with(response)
        .mount(s)
        .await;
}

fn service(s: &MockServer, jql: &str) -> Services {
    Services {
        github: None,
        jira: Some(JiraService {
            client: JiraClient::new(&s.uri(), "mira@firma.example", Secret::new("jt")).unwrap(),
            jql: jql.into(),
            in_project: false,
            fields: Default::default(),
        }),
        ..Default::default()
    }
}

use wiremock::matchers::path_regex;

#[tokio::test]
async fn the_default_query_asks_for_active_sprints_and_reads_sprint_and_status() {
    let s = site(sprint_fields()).await;
    answer_jql(
        &s,
        SPRINT_JQL,
        ResponseTemplate::new(200).set_body_json(json!({"issues": [
            // Carried over: closed sprint and the active one – the active one counts.
            issue("BAU-1", json!([sprint("Sprint 6", "closed"), sprint("Sprint 7", "active")]), "indeterminate"),
            issue("BAU-2", json!(null), "done"),
        ]})),
    )
    .await;
    // The sprint field is asked for.
    let jql = jql_for(&["BAU".into()], None).unwrap();
    assert_eq!(jql, SPRINT_JQL);
    let (ws, _) = sync(&service(&s, &jql), &Workspace::default(), NOW_MS).await;

    let one = ws.item_by_key("BAU-1").unwrap();
    let sp = one.sprint.as_ref().unwrap();
    assert_eq!(
        (sp.name.as_str(), sp.state),
        ("Sprint 7", SprintState::Active)
    );
    assert_eq!(sp.end, "2024-05-20T08:00:00.000Z");
    assert_eq!(one.status, cockpit_core::TicketStatus::InProgress);
    let two = ws.item_by_key("BAU-2").unwrap();
    assert!(two.sprint.is_none());
    assert_eq!(two.status, cockpit_core::TicketStatus::Done);
    assert!(two.updated_ms.is_some());
    let st = ws.sync.state_of(Src::Jira).unwrap();
    assert!(
        st.is_ok() && st.warning_lines(Lang::De).is_empty(),
        "{st:?}"
    );
    // Not assigned to me: shown because of the sprint, not because of a watch.
    assert_eq!(one.reason_line().render(Lang::De), "Im Sprint");
}

#[tokio::test]
async fn the_sprint_field_is_asked_for_in_the_fields_list() {
    let s = site(sprint_fields()).await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(wiremock::matchers::query_param_contains(
            "fields",
            "customfield_10020",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": []})))
        .expect(1..)
        .mount(&s)
        .await;
    // No active sprint and nothing open either: the fallback runs and finds nothing.
    answer_jql(
        &s,
        OPEN_JQL,
        ResponseTemplate::new(200).set_body_json(json!({"issues": []})),
    )
    .await;
    let (ws, _) = sync(&service(&s, SPRINT_JQL), &Workspace::default(), NOW_MS).await;
    assert!(ws.sync.state_of(Src::Jira).unwrap().is_ok());
}

#[tokio::test]
async fn a_rejected_sprint_query_falls_back_to_all_open_tickets_with_a_hint() {
    let s = site(json!([{"id": "summary", "name": "Summary"}])).await;
    answer_jql(
        &s,
        SPRINT_JQL,
        ResponseTemplate::new(400).set_body_json(
            json!({"errorMessages": ["Die Funktion 'openSprints' ist ungültig (sprint)"], "errors": {}}),
        ),
    )
    .await;
    answer_jql(
        &s,
        OPEN_JQL,
        ResponseTemplate::new(200)
            .set_body_json(json!({"issues": [issue("BAU-3", json!(null), "new")]})),
    )
    .await;
    let (ws, _) = sync(&service(&s, SPRINT_JQL), &Workspace::default(), NOW_MS).await;
    assert!(ws.item_by_key("BAU-3").is_some());
    let st = ws.sync.state_of(Src::Jira).unwrap();
    assert!(st.is_ok());
    assert_eq!(
        st.warning_lines(Lang::De),
        ["Dieses Projekt hat keine Sprints – zeige alle offenen Tickets"]
    );
    // The sprint filter then falls back to "All open", so the ticket is not hidden.
    let shown = ws.inbox(
        cockpit_core::Filter::All,
        cockpit_core::SourceFilter::All,
        "",
        NOW_MS,
        &cockpit_core::InboxFilter {
            ownership: cockpit_core::Ownership::All,
            ..Default::default()
        },
    );
    assert_eq!(shown.len(), 1);
}

#[tokio::test]
async fn no_active_sprint_but_closed_ones_says_so() {
    let s = site(sprint_fields()).await;
    answer_jql(
        &s,
        SPRINT_JQL,
        ResponseTemplate::new(200).set_body_json(json!({"issues": []})),
    )
    .await;
    answer_jql(
        &s,
        OPEN_JQL,
        ResponseTemplate::new(200).set_body_json(
            json!({"issues": [issue("BAU-4", json!([sprint("Sprint 6", "closed")]), "new")]}),
        ),
    )
    .await;
    let (ws, _) = sync(&service(&s, SPRINT_JQL), &Workspace::default(), NOW_MS).await;
    assert_eq!(
        ws.sync.state_of(Src::Jira).unwrap().warning_lines(Lang::De),
        ["Kein aktiver Sprint – zeige alle offenen Tickets"]
    );
    // The ticket's sprint is closed: "Current sprint" hides it, "All open" shows it.
    let by = |sprint| cockpit_core::InboxFilter {
        ownership: cockpit_core::Ownership::All,
        sprint,
        ..Default::default()
    };
    let list = |f: &cockpit_core::InboxFilter| {
        ws.inbox(
            cockpit_core::Filter::All,
            cockpit_core::SourceFilter::All,
            "",
            NOW_MS,
            f,
        )
        .len()
    };
    assert_eq!(list(&by(cockpit_core::SprintView::Current)), 0);
    assert_eq!(list(&by(cockpit_core::SprintView::AllOpen)), 1);
}

#[tokio::test]
async fn an_error_of_a_query_set_by_hand_is_an_error_not_an_empty_list() {
    let s = site(sprint_fields()).await;
    let custom = "project = BAU AND sprint in openSprints()";
    assert_eq!(open_tickets_jql(custom), None, "never rewritten");
    answer_jql(
        &s,
        custom,
        ResponseTemplate::new(400)
            .set_body_json(json!({"errorMessages": ["Fehler in der JQL (sprint)"]})),
    )
    .await;
    let (ws, _) = sync(&service(&s, custom), &Workspace::default(), NOW_MS).await;
    assert!(matches!(
        ws.sync.state_of(Src::Jira).unwrap().problem,
        Some(Problem::Other(_))
    ));
    // Also an unrelated 400 of the default query stays an error.
    let s = site(sprint_fields()).await;
    answer_jql(
        &s,
        SPRINT_JQL,
        ResponseTemplate::new(400)
            .set_body_json(json!({"errorMessages": ["Das Projekt BAU gibt es nicht"]})),
    )
    .await;
    let (ws, _) = sync(&service(&s, SPRINT_JQL), &Workspace::default(), NOW_MS).await;
    assert!(ws.sync.state_of(Src::Jira).unwrap().problem.is_some());
}

#[tokio::test]
async fn the_sprint_field_is_found_by_type_then_by_name_and_may_be_missing() {
    // By name only.
    let s = site(json!([{"id": "customfield_10020", "name": "Sprint"}])).await;
    let c = JiraClient::new(&s.uri(), "a@b.de", Secret::new("t")).unwrap();
    assert_eq!(
        c.sprint_field().await.unwrap().as_deref(),
        Some("customfield_10020")
    );
    // Not at all (no Jira Software).
    let s = site(json!([{"id": "summary", "name": "Summary"}])).await;
    let c = JiraClient::new(&s.uri(), "a@b.de", Secret::new("t")).unwrap();
    assert_eq!(c.sprint_field().await.unwrap(), None);
    // By type, even if another field is named „Sprint“ first.
    let s = site(json!([
        {"id": "customfield_1", "name": "Sprint"},
        {"id": "customfield_2", "name": "Iteration",
         "schema": {"custom": "com.pyxis.greenhopper.jira:gh-sprint"}}
    ]))
    .await;
    let c = JiraClient::new(&s.uri(), "a@b.de", Secret::new("t")).unwrap();
    assert_eq!(
        c.sprint_field().await.unwrap().as_deref(),
        Some("customfield_2")
    );
}

#[test]
fn sprint_values_of_both_forms_are_read() {
    let s = sprint_of(&json!([sprint("S1", "closed"), sprint("S2", "future")])).unwrap();
    assert_eq!((s.name.as_str(), s.state), ("S2", SprintState::Future));
    let s = sprint_of(&json!([sprint("S1", "closed"), sprint("S0", "closed")])).unwrap();
    assert_eq!(s.name, "S0", "the last closed one");
    let old = json!([
        "com.atlassian.greenhopper.service.sprint.Sprint@1f[id=7,state=ACTIVE,name=Sprint 7,startDate=2024-05-06T08:00:00.000Z,endDate=2024-05-20T08:00:00.000Z]"
    ]);
    let s = sprint_of(&old).unwrap();
    assert_eq!(
        (s.name.as_str(), s.state),
        ("Sprint 7", SprintState::Active)
    );
    assert_eq!(sprint_of(&json!(null)), None);
    assert_eq!(sprint_of(&json!([])), None);
    assert_eq!(sprint_of(&json!([{"name": "x", "state": "weird"}])), None);
}

#[test]
fn the_fallback_only_rewrites_the_default_queries() {
    assert_eq!(
        open_tickets_jql("sprint in openSprints() ORDER BY updated DESC").unwrap(),
        "statusCategory != Done ORDER BY updated DESC"
    );
    assert_eq!(
        open_tickets_jql("project = BAU AND sprint in openSprints() ORDER BY updated DESC")
            .unwrap(),
        "project = BAU AND statusCategory != Done ORDER BY updated DESC"
    );
    assert_eq!(open_tickets_jql("assignee = currentUser()"), None);
    assert_eq!(
        open_tickets_jql("sprint in openSprints() AND assignee = currentUser()"),
        None
    );
}
