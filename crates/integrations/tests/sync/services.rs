use super::*;
use cockpit_i18n::Lang;

#[tokio::test]
async fn jira_is_read_page_by_page() {
    let s = MockServer::start().await;
    get(&s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    let issue = |n: u32| {
        json!({"key": format!("ABC-{n}"), "fields": {"summary": format!("T{n}"),
            "status": {"name": "To Do", "statusCategory": {"key": "new"}},
            "reporter": {"accountId": "x", "displayName": "Lena Koch"}, "updated": "2024-05-14T10:00:00Z"}})
    };
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("nextPageToken", "seite2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": [issue(3)]})))
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"issues": [issue(1), issue(2)], "nextPageToken": "seite2"})),
        )
        .mount(&s)
        .await;
    for n in 1..=3 {
        get(
            &s,
            &format!("/rest/api/3/issue/ABC-{n}/comment"),
            ok(r#"{"comments":[]}"#),
        )
        .await;
    }
    let (ws, _) = sync(&services(None, Some(&s)), &Workspace::default(), NOW_MS).await;
    let keys: Vec<&str> = ws.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
}

#[tokio::test]
async fn jira_in_project_mode() {
    let j = jira_server().await;
    let mut d = services(None, Some(&j));
    if let Some(jira) = d.jira.as_mut() {
        jira.jql = jql_for_project("BAU").unwrap();
        jira.in_project = true;
    }
    let (ws, _) = sync(&d, &Workspace::default(), NOW_MS).await;
    let mine = ws.item_by_key("BAU-142").unwrap();
    assert_eq!(mine.reason_line().render(Lang::De), "Dir zugewiesen");
    let other: Vec<_> = ws.items.iter().filter(|i| i.key != "BAU-142").collect();
    assert!(!other.is_empty());
    assert!(
        other
            .iter()
            .all(|i| i.reason_line().render(Lang::De) == "Im Projekt"),
        "{other:?}"
    );
}

#[tokio::test]
async fn github_comments_page_by_page() {
    // 100 comments on page 1, one on page 2.
    let g = github_server().await;
    let entry = |i: u32| json!({"user": {"login": "mkeller"}, "body": format!("k{i}"), "created_at": "2024-05-14T09:00:00Z"});
    let page1: Vec<_> = (0..100).map(entry).collect();
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/issues/91/comments")))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([entry(100)])))
        .with_priority(1)
        .mount(&g)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/issues/91/comments")))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page1))
        .with_priority(1)
        .mount(&g)
        .await;
    let (ws, _) = sync(&services(Some(&g), None), &Workspace::default(), NOW_MS).await;
    assert_eq!(
        ws.item("gh-acme-baukasten-91").unwrap().comments.len(),
        101 + 1
    );
}

#[test]
fn jql_from_projects_and_own_query() {
    assert_eq!(
        jql_for(&["BAU".into(), "WEB".into()], None).unwrap(),
        "sprint in openSprints() AND project in (BAU, WEB) ORDER BY updated DESC"
    );
    assert_eq!(
        jql_for(&["BAU".into()], Some(" project = X ")).unwrap(),
        "project = X"
    );
    assert!(jql_for(&["BAU) OR (x".into()], None).is_err());
    assert_eq!(
        jql_for(&[], None).unwrap(),
        "sprint in openSprints() ORDER BY updated DESC"
    );
}

#[test]
fn merge_stays_pure_and_deterministic() {
    let a = sync::merge(&Workspace::default(), &Remote::default(), 1);
    let b = sync::merge(&Workspace::default(), &Remote::default(), 1);
    assert_eq!(a, b);
}
