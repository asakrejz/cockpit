//! Jira: fields with their names, attachments, images in the texts, and the link to the ticket.

use super::*;
use cockpit_core::DetailValue;
use cockpit_core::adf::Block;

fn service(s: &MockServer, site: Option<&str>) -> Services {
    let client = JiraClient::new(&s.uri(), "mira@firma.example", Secret::new("jt")).unwrap();
    Services {
        github: None,
        jira: Some(JiraService {
            client: client.with_site(site),
            jql: jql_for(&[], None).unwrap(),
            in_project: false,
            fields: Default::default(),
        }),
        ..Default::default()
    }
}

async fn server() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    get(&s, "/rest/api/3/field", ok("[]")).await;
    // The search must ask for all fields with their names.
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("expand", "names"))
        .and(wiremock::matchers::query_param_contains("fields", "*all"))
        .respond_with(ok(fixture!("jira_search_details.json")))
        .mount(&s)
        .await;
    get(
        &s,
        "/rest/api/3/issue/BAU-142/comment",
        ok(r#"{"comments":[]}"#),
    )
    .await;
    s
}

async fn load(services: &Services) -> cockpit_core::Item {
    let remote = services.fetch(&Workspace::default(), NOW_MS, &[]).await;
    let Outcome::Loaded(loaded) = &remote.sources[0].outcome else {
        panic!("not loaded: {:?}", remote.sources[0].outcome)
    };
    loaded.items[0].clone()
}

#[tokio::test]
async fn fields_come_with_their_names_and_empty_ones_stay_out() {
    let s = server().await;
    let item = load(&service(&s, None)).await;
    let d = item.details.expect("details");
    let ids: Vec<&str> = d.fields.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "status",
            "issuetype",
            "priority",
            "assignee",
            "reporter",
            "labels",
            "components",
            "story_points",
            "created",
            "updated",
            "customfield_10100",
            "customfield_10101"
        ]
    );
    let by = |id: &str| d.fields.iter().find(|f| f.id == id).unwrap();
    assert_eq!(by("customfield_10100").name, "Acceptance criteria");
    assert!(matches!(&by("customfield_10100").value, DetailValue::Rich(doc) if !doc.is_empty()));
    assert_eq!(
        by("customfield_10101").value,
        DetailValue::Text("Design System".into())
    );
    assert_eq!(by("story_points").value, DetailValue::Text("3".into()));
    // `fixVersions` is empty and `customfield_10103` is null: neither shows.
    assert!(!ids.contains(&"fixVersions") && !ids.contains(&"customfield_10103"));
}

#[tokio::test]
async fn attachments_and_images_in_the_description() {
    let s = server().await;
    let item = load(&service(&s, None)).await;
    let d = item.details.unwrap();
    assert_eq!(d.attachments.len(), 2);
    assert_eq!(
        (
            d.attachments[0].name.as_str(),
            d.attachments[0].size,
            d.attachments[0].thumbnail
        ),
        ("mockup.png", 2048, true)
    );
    // The picture of the description is linked to its attachment, the PDF stays a placeholder.
    let media: Vec<Option<String>> = item
        .description_rich
        .unwrap()
        .iter()
        .filter_map(|b| match b {
            Block::Media { attachment, .. } => Some(attachment.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(media, [Some("10001".to_string()), None]);
}

#[tokio::test]
async fn the_ticket_links_to_the_site_when_it_is_known() {
    let s = server().await;
    let item = load(&service(&s, Some("https://acme.atlassian.net"))).await;
    assert_eq!(
        item.url.as_deref(),
        Some("https://acme.atlassian.net/browse/BAU-142")
    );
    assert_eq!(item.web_url(), item.url);
    // Without a site there is no link.
    let item = load(&service(&s, None)).await;
    assert_eq!(item.url, None);
    assert_eq!(item.web_url(), None);
}
