//! The query that follows the selection in the app: a project or a sprint view other than the
//! active sprint fetches all open tickets, also those of other people. Only ever read queries,
//! answered by wiremock.

use super::*;
use cockpit_core::{Scope, SprintView};
use cockpit_integrations::jira::{JiraQuery, jql_for_selection};

fn ticket(key: &str, assignee: serde_json::Value) -> serde_json::Value {
    json!({"key": key, "fields": {
        "summary": format!("T {key}"),
        "status": {"name": "Status", "statusCategory": {"key": "indeterminate"}},
        "assignee": assignee,
        "updated": "2024-05-14T10:00:00Z"
    }})
}

async fn site(jql: &str, issues: serde_json::Value) -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    get(&s, "/rest/api/3/field", ok("[]")).await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("jql", jql))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "issues": issues })))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::path_regex(
            r"^/rest/api/3/issue/.*/comment$",
        ))
        .respond_with(ok(r#"{"comments":[]}"#))
        .mount(&s)
        .await;
    s
}

fn services(s: &MockServer, query: JiraQuery) -> Services {
    Services {
        github: None,
        jira: Some(JiraService {
            client: JiraClient::new(&s.uri(), "mira@firma.example", Secret::new("jt")).unwrap(),
            jql: query.jql,
            in_project: query.in_selection,
            fields: Default::default(),
        }),
        ..Default::default()
    }
}

fn query(chosen: Option<&str>, view: SprintView) -> JiraQuery {
    jql_for_selection(&["WEB".into()], None, chosen, view).unwrap()
}

#[test]
fn nothing_chosen_keeps_the_query_of_the_settings() {
    let q = query(None, SprintView::Current);
    assert_eq!(
        q.jql,
        "sprint in openSprints() AND project in (WEB) ORDER BY updated DESC"
    );
    assert!(!q.in_selection);
    // A JQL set by hand stays, as long as no project is chosen.
    let own = jql_for_selection(
        &[],
        Some(" assignee = currentUser() "),
        None,
        SprintView::AllOpen,
    );
    assert_eq!(own.unwrap().jql, "assignee = currentUser()");
    let chosen = jql_for_selection(&[], Some("assignee = x"), Some("BAU"), SprintView::AllOpen);
    assert!(chosen.unwrap().jql.starts_with("project in (BAU) AND"));
}

#[test]
fn a_project_with_the_current_sprint_is_the_sprint_of_the_project() {
    let q = query(Some("BAU"), SprintView::Current);
    assert_eq!(
        q.jql,
        "project = BAU AND sprint in openSprints() ORDER BY updated DESC"
    );
    assert!(q.in_selection);
}

#[test]
fn the_other_sprint_views_ask_for_open_tickets_of_everybody() {
    assert_eq!(
        query(Some("BAU"), SprintView::AllOpen).jql,
        "project in (BAU) AND statusCategory != Done ORDER BY updated DESC"
    );
    assert_eq!(
        query(None, SprintView::AllOpen).jql,
        "project in (WEB) AND statusCategory != Done ORDER BY updated DESC",
        "without a chosen project the projects of the settings apply"
    );
    assert_eq!(
        query(Some("BAU"), SprintView::Backlog).jql,
        "project in (BAU) AND statusCategory != Done AND sprint is EMPTY ORDER BY updated DESC"
    );
    assert_eq!(
        query(Some("BAU"), SprintView::Recent).jql,
        "project in (BAU) AND statusCategory != Done AND updated >= -14d ORDER BY updated DESC"
    );
    let all = jql_for_selection(&[], None, None, SprintView::AllOpen).unwrap();
    assert_eq!(all.jql, "statusCategory != Done ORDER BY updated DESC");
    for q in [&all, &query(Some("BAU"), SprintView::Backlog)] {
        assert!(
            !q.jql.contains("currentUser") && !q.jql.contains("assignee"),
            "no restriction to my own tickets: {}",
            q.jql
        );
        assert!(q.in_selection);
    }
}

#[test]
fn project_keys_are_checked_so_nothing_slips_into_the_query() {
    assert!(jql_for_selection(&[], None, Some("A) OR (B"), SprintView::AllOpen).is_err());
    assert!(jql_for_selection(&["x y".into()], None, None, SprintView::AllOpen).is_err());
}

#[tokio::test]
async fn all_open_tickets_arrive_including_those_of_other_people() {
    let q = query(Some("BAU"), SprintView::AllOpen);
    let s = site(
        &q.jql,
        json!([
            ticket(
                "BAU-1",
                json!({"accountId": "acc-mira", "displayName": "Mira Keller"})
            ),
            ticket(
                "BAU-2",
                json!({"accountId": "acc-tobias", "displayName": "Tobias Lang"})
            ),
            ticket("BAU-3", json!(null)),
        ]),
    )
    .await;
    let (ws, _) = sync(&services(&s, q), &Workspace::default(), NOW_MS).await;

    assert!(ws.sync.state_of(Source::Jira).unwrap().is_ok());
    let mine = ws.item_by_key("BAU-1").unwrap();
    assert_eq!(mine.scope, Scope::Mine);
    let other = ws.item_by_key("BAU-2").unwrap();
    assert_eq!(
        (other.scope, other.owner.name.as_str()),
        (Scope::Team, "Tobias Lang")
    );
    assert_eq!(other.reason_line().de(), "Im Projekt");
    let nobody = ws.item_by_key("BAU-3").unwrap();
    assert!(nobody.owner.nobody);
    assert_eq!(nobody.scope, Scope::Team);
    // They are in the inbox once „Alle“ and „Alle offenen“ are chosen.
    let by = cockpit_core::InboxFilter {
        ownership: cockpit_core::Ownership::All,
        sprint: SprintView::AllOpen,
        ..Default::default()
    };
    let keys: Vec<_> = ws
        .inbox(
            cockpit_core::Filter::All,
            cockpit_core::SourceFilter::All,
            "",
            NOW_MS,
            &by,
        )
        .iter()
        .map(|i| i.key.clone())
        .collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
}

#[tokio::test]
async fn without_a_selection_the_sprint_query_stays_as_before() {
    let q = query(None, SprintView::Current);
    let s = site(
        &q.jql,
        json!([ticket(
            "WEB-1",
            json!({"accountId": "acc-tobias", "displayName": "Tobias Lang"})
        )]),
    )
    .await;
    let (ws, _) = sync(&services(&s, q), &Workspace::default(), NOW_MS).await;
    assert_eq!(
        ws.item_by_key("WEB-1").unwrap().reason_line().de(),
        "Im Sprint"
    );
}
