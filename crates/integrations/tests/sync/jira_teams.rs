//! Jira: the Atlassian „Team“ field – detection, the override from the configuration, the team
//! of each ticket, and sites without the field.

use super::*;
use cockpit_core::TeamRef;
use wiremock::matchers::path_regex;

const JQL: &str = "project in (APP) ORDER BY updated DESC";

fn issue(key: &str, team: serde_json::Value) -> serde_json::Value {
    json!({"key": key, "fields": {
        "summary": format!("T {key}"),
        "status": {"name": "Status", "statusCategory": {"key": "indeterminate"}},
        "assignee": {"accountId": "x", "displayName": "Lena Koch"},
        "updated": "2024-05-14T10:00:00Z",
        "customfield_10400": team
    }})
}

fn team(id: &str, name: &str) -> serde_json::Value {
    json!({"id": id, "name": name, "avatarUrl": "https://example.invalid/a.png", "isVisible": true})
}

async fn site(fields: serde_json::Value, issues: Vec<serde_json::Value>) -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    get(&s, "/rest/api/3/field", ok(&fields.to_string())).await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "issues": issues })))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/rest/api/3/issue/.*/comment$"))
        .respond_with(ok(r#"{"comments":[]}"#))
        .mount(&s)
        .await;
    s
}

fn team_fields() -> serde_json::Value {
    json!([
        {"id": "summary", "name": "Summary", "schema": {"type": "string"}},
        {"id": "customfield_10399", "name": "Team (alt)", "schema": {"type": "string"}},
        {"id": "customfield_10400", "name": "Team",
         "schema": {"type": "team", "custom": "com.atlassian.teams:rm-teams-custom-field-team"}}
    ])
}

fn client(s: &MockServer) -> JiraClient {
    JiraClient::new(&s.uri(), "mira@firma.example", Secret::new("jt")).unwrap()
}

async fn load(c: JiraClient) -> Result<cockpit_core::Loaded, cockpit_integrations::Error> {
    cockpit_integrations::sync::jira_load(&c, JQL, false, &Default::default(), 1_715_688_000).await
}

fn team_of(l: &cockpit_core::Loaded, key: &str) -> Option<TeamRef> {
    l.items.iter().find(|i| i.key == key).unwrap().team.clone()
}

#[tokio::test]
async fn the_team_field_is_found_by_its_type_and_read_into_each_ticket() {
    let s = site(
        team_fields(),
        vec![
            issue("APP-1", team("t-1", "Design System")),
            issue("APP-2", team("t-2", "Web")),
            issue("APP-3", json!(null)),
        ],
    )
    .await;
    assert_eq!(
        client(&s).team_field().await.unwrap().as_deref(),
        Some("customfield_10400")
    );
    let loaded = load(client(&s)).await.unwrap();
    assert_eq!(
        team_of(&loaded, "APP-1"),
        Some(TeamRef::new("t-1", "Design System"))
    );
    assert_eq!(team_of(&loaded, "APP-2"), Some(TeamRef::new("t-2", "Web")));
    // No team is „no team“, not an error.
    assert_eq!(team_of(&loaded, "APP-3"), None);
    assert!(loaded.warnings.is_empty());
}

#[tokio::test]
async fn the_older_team_type_and_the_name_are_found_too() {
    let by_type = json!([{"id": "customfield_7", "name": "Gruppe",
        "schema": {"type": "any", "custom": "com.atlassian.jira.plugin.system.customfieldtypes:atlassian-team"}}]);
    let s = site(by_type, vec![]).await;
    assert_eq!(
        client(&s).team_field().await.unwrap().as_deref(),
        Some("customfield_7")
    );
    let by_name = json!([{"id": "customfield_8", "name": "Team", "schema": {"type": "string"}}]);
    let s = site(by_name, vec![]).await;
    assert_eq!(
        client(&s).team_field().await.unwrap().as_deref(),
        Some("customfield_8")
    );
}

#[tokio::test]
async fn the_configured_field_replaces_the_detection() {
    // No field list at all (404): the override does not ask for it.
    let s = site(json!([]), vec![issue("APP-1", team("t-9", "Plattform"))]).await;
    let c = client(&s).with_team_field(Some(" customfield_10400 "));
    assert_eq!(
        c.team_field().await.unwrap().as_deref(),
        Some("customfield_10400")
    );
    let loaded = load(c).await.unwrap();
    assert_eq!(
        team_of(&loaded, "APP-1"),
        Some(TeamRef::new("t-9", "Plattform"))
    );
    // An empty value means „detect“.
    let c = client(&s).with_team_field(Some("  "));
    assert_eq!(c.team_field().await.unwrap(), None);
}

#[tokio::test]
async fn a_site_without_the_field_gives_tickets_without_a_team() {
    let fields = json!([{"id": "summary", "name": "Summary", "schema": {"type": "string"}}]);
    let s = site(fields, vec![issue("APP-1", team("t-1", "Ignored"))]).await;
    assert_eq!(client(&s).team_field().await.unwrap(), None);
    let loaded = load(client(&s)).await.unwrap();
    assert_eq!(team_of(&loaded, "APP-1"), None);
    assert!(loaded.warnings.is_empty());
}

#[tokio::test]
async fn the_search_asks_for_the_team_field() {
    let s = site(team_fields(), vec![issue("APP-1", team("t-1", "Web"))]).await;
    load(client(&s)).await.unwrap();
    let asked = s.received_requests().await.unwrap();
    let search = asked
        .iter()
        .find(|r| r.url.path() == "/rest/api/3/search/jql")
        .unwrap();
    let fields = search
        .url
        .query_pairs()
        .find(|(k, _)| k == "fields")
        .unwrap()
        .1
        .to_string();
    assert!(fields.contains("customfield_10400"), "{fields}");
}

#[tokio::test]
async fn an_unreadable_field_list_is_a_warning_and_the_tickets_still_load() {
    let s = MockServer::start().await;
    get(&s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/field"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"issues": [issue("APP-1", team("t-1", "Web"))]})),
        )
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/rest/api/3/issue/.*/comment$"))
        .respond_with(ok(r#"{"comments":[]}"#))
        .mount(&s)
        .await;
    let loaded = load(client(&s)).await.unwrap();
    assert_eq!(loaded.items.len(), 1);
    assert_eq!(team_of(&loaded, "APP-1"), None);
    assert!(!loaded.warnings.is_empty());
}

#[test]
fn team_values_in_other_shapes() {
    use cockpit_integrations::jira::team_of;
    assert_eq!(team_of(&json!(null)), None);
    assert_eq!(team_of(&json!({})), None);
    assert_eq!(team_of(&json!("")), None);
    assert_eq!(team_of(&json!([])), None);
    // Only the ID: it serves as the name.
    assert_eq!(team_of(&json!("abc")), Some(TeamRef::new("abc", "abc")));
    assert_eq!(
        team_of(&json!({"id": "abc"})),
        Some(TeamRef::new("abc", "abc"))
    );
}
