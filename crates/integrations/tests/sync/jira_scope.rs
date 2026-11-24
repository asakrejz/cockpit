//! Everybody's tickets: the query holds no restriction to my own tickets, pages are followed,
//! and a cut is reported. Only read queries, answered by wiremock.

use super::*;
use cockpit_core::{Scope, SprintView};
use cockpit_integrations::jira::{MAX_ISSUES, jql_for_selection};

fn issue(n: usize, assignee: serde_json::Value) -> serde_json::Value {
    json!({"key": format!("BAU-{n}"), "fields": {
        "summary": format!("Ticket {n}"),
        "status": {"name": "Open", "statusCategory": {"key": "new"}},
        "assignee": assignee,
        "reporter": {"accountId": "other", "displayName": "Lena"},
        "priority": {"name": "High"},
        "issuetype": {"name": "Bug"},
        "watches": {"isWatching": n.is_multiple_of(2)},
        "updated": "2024-05-14T10:00:00Z"
    }})
}

async fn common(s: &MockServer) {
    get(s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    get(s, "/rest/api/3/field", ok("[]")).await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::path_regex(
            r"^/rest/api/3/issue/.*/comment$",
        ))
        .respond_with(ok(r#"{"comments":[]}"#))
        .mount(s)
        .await;
}

fn service(s: &MockServer, jql: &str) -> Services {
    Services {
        github: None,
        jira: Some(JiraService {
            client: JiraClient::new(&s.uri(), "mira@firma.example", Secret::new("jt")).unwrap(),
            jql: jql.to_string(),
            in_project: true,
            fields: Default::default(),
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_project_loads_the_tickets_of_everybody_with_reporter_priority_and_watching() {
    let s = MockServer::start().await;
    common(&s).await;
    let q = jql_for_selection(&[], None, Some("BAU"), SprintView::AllOpen).unwrap();
    assert!(!q.jql.contains("currentUser"), "{}", q.jql);
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("jql", q.jql.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": [
            issue(1, json!({"accountId": "x1", "displayName": "Jonas"})),
            issue(2, json!(null)),
        ]})))
        .expect(1)
        .mount(&s)
        .await;
    let loaded = service(&s, &q.jql)
        .fetch(&Workspace::default(), NOW_MS, &[])
        .await;
    let Outcome::Loaded(l) = &loaded.sources[0].outcome else {
        panic!("not loaded: {:?}", loaded.sources[0].outcome)
    };
    assert_eq!(l.items.len(), 2);
    assert!(l.items.iter().all(|i| i.scope == Scope::Team));
    assert_eq!(l.items[0].owner.name, "Jonas");
    assert!(l.items[1].owner.nobody);
    assert_eq!(l.items[0].reporter.as_deref(), Some("Lena"));
    assert_eq!(l.items[0].priority, cockpit_core::Priority::High);
    assert_eq!(l.items[0].issue_type.as_deref(), Some("Bug"));
    assert!(!l.items[0].watching && l.items[1].watching);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[tokio::test]
async fn every_page_is_followed_and_a_cut_is_reported() {
    let s = MockServer::start().await;
    common(&s).await;
    let jql = "project in (BAU) AND statusCategory != Done ORDER BY updated DESC";
    let per_page = 50;
    let pages = MAX_ISSUES.div_ceil(per_page);
    for p in 0..pages {
        let issues: Vec<_> = (0..per_page)
            .map(|n| issue(p * per_page + n, json!(null)))
            .collect();
        let mut body = json!({ "issues": issues });
        // Jira has one more page after the last one we fetch.
        body["nextPageToken"] = json!(format!("t{}", p + 1));
        let mut mock = Mock::given(method("GET"))
            .and(path("/rest/api/3/search/jql"))
            .and(query_param("jql", jql));
        mock = if p == 0 {
            mock.and(wiremock::matchers::query_param_is_missing("nextPageToken"))
        } else {
            mock.and(query_param("nextPageToken", format!("t{p}").as_str()))
        };
        mock.respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&s)
            .await;
    }
    let loaded = service(&s, jql)
        .fetch(&Workspace::default(), NOW_MS, &[])
        .await;
    let Outcome::Loaded(l) = &loaded.sources[0].outcome else {
        panic!("not loaded")
    };
    assert_eq!(l.items.len(), MAX_ISSUES);
    assert_eq!(l.warnings.len(), 1, "the cut is reported once");
}

#[tokio::test]
async fn the_last_page_ends_the_search_without_a_hint() {
    let s = MockServer::start().await;
    common(&s).await;
    let jql = "project in (BAU) AND statusCategory != Done ORDER BY updated DESC";
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("jql", jql))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issues": [issue(1, json!(null))],
            "isLast": true
        })))
        .expect(1)
        .mount(&s)
        .await;
    let loaded = service(&s, jql)
        .fetch(&Workspace::default(), NOW_MS, &[])
        .await;
    let Outcome::Loaded(l) = &loaded.sources[0].outcome else {
        panic!("not loaded")
    };
    assert_eq!(l.items.len(), 1);
    assert!(l.warnings.is_empty());
}
