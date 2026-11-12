//! Jira: Abbildung und Client gegen nachgebaute Antworten (`tests/fixtures`).

use cockpit_core::{Kind, ReasonKind, Scope, Source, TicketStatus};
use cockpit_integrations::jira::{
    ASSIGNED_OR_WATCHED_JQL, JiraClient, comments_to_core, dto::*, issue_to_item, jql_fuer_projekt,
};
use cockpit_integrations::{Fehler, Secret, zeit};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("fixtures/", $name))
    };
}

/// 2024-05-14T12:00:00Z
fn jetzt() -> i64 {
    zeit::parse_iso("2024-05-14T12:00:00Z").unwrap()
}

fn suche() -> SearchResult {
    serde_json::from_str(fixture!("jira_search.json")).unwrap()
}

#[test]
fn zugewiesenes_issue() {
    let s = suche();
    let item = issue_to_item(&s.issues[0], "acc-mira", jetzt());
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
}

#[test]
fn beobachtetes_issue_im_review() {
    let s = suche();
    let item = issue_to_item(&s.issues[1], "acc-mira", jetzt());
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
fn issue_ohne_bearbeiter_und_offen() {
    let s = suche();
    let item = issue_to_item(&s.issues[2], "acc-mira", jetzt());
    assert_eq!(item.status, TicketStatus::Open);
    // Ohne Bearbeiter gilt der Melder als Besitzer.
    assert_eq!(item.owner.name, "Tobias Lang");

    let mut ohne_alles = s.issues[2].clone();
    ohne_alles.fields.reporter = None;
    assert_eq!(
        issue_to_item(&ohne_alles, "acc-mira", jetzt()).owner.name,
        "Nicht zugewiesen"
    );
}

#[test]
fn erledigt_wird_erkannt() {
    let mut issue = suche().issues.remove(0);
    issue.fields.status.status_category = Some(StatusCategory { key: "done".into() });
    assert_eq!(
        issue_to_item(&issue, "x", jetzt()).status,
        TicketStatus::Done
    );
    issue.fields.status.status_category = None;
    assert_eq!(
        issue_to_item(&issue, "x", jetzt()).status,
        TicketStatus::Open
    );
}

#[test]
fn kommentare_mit_adf_und_eigenen() {
    let page: CommentPage = serde_json::from_str(fixture!("jira_comments_142.json")).unwrap();
    let c = comments_to_core(&page.comments, "acc-mira", jetzt());
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].text, "@Du kannst du das übernehmen?");
    assert_eq!(c[0].author.name, "Tobias Lang");
    assert!(!c[0].own);
    assert_eq!(c[1].text, "Ja, bin dran.");
    assert!(c[1].own);
    assert_eq!(c[1].time, "vor 4 Std.");
}

// ---------- Client gegen Mock-Server ----------

fn antwort(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "application/json")
}

#[tokio::test]
async fn inbox_laden_mit_jql_und_anmeldung() {
    let server = MockServer::start().await;
    // base64("mira@firma.example:jira-token")
    let basic = "Basic bWlyYUBmaXJtYS5leGFtcGxlOmppcmEtdG9rZW4=";
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .and(header("authorization", basic))
        .respond_with(antwort(fixture!("jira_myself.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("jql", ASSIGNED_OR_WATCHED_JQL))
        .and(header("authorization", basic))
        .respond_with(antwort(fixture!("jira_search.json")))
        .mount(&server)
        .await;

    let c = JiraClient::new(
        &server.uri(),
        "mira@firma.example",
        Secret::new("jira-token"),
    )
    .unwrap();
    let items = c.lade_inbox(jetzt()).await.unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].scope, Scope::Mine);
    assert_eq!(items[1].scope, Scope::Team);
}

#[tokio::test]
async fn kommentare_und_fehler() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/BAU-142/comment"))
        .respond_with(antwort(fixture!("jira_comments_142.json")))
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
    assert_eq!(c.kommentare("BAU-142").await.unwrap().len(), 2);
    assert_eq!(
        c.kommentare("BAU-1").await.unwrap_err(),
        Fehler::NichtGefunden
    );
    assert_eq!(c.ich().await.unwrap_err(), Fehler::NichtAngemeldet);
}

#[tokio::test]
async fn projekte_seitenweise() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/project/search"))
        .and(query_param("startAt", "0"))
        .and(query_param("orderBy", "name"))
        .respond_with(antwort(fixture!("jira_projects_1.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/project/search"))
        .and(query_param("startAt", "2"))
        .respond_with(antwort(fixture!("jira_projects_2.json")))
        .expect(1)
        .mount(&server)
        .await;

    let c = JiraClient::new(&server.uri(), "a@b.de", Secret::new("t")).unwrap();
    let projekte = c.projekte().await.unwrap();
    let keys: Vec<_> = projekte.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["BAU", "OPS", "WEB"]);
    assert_eq!(projekte[0].name, "Baukasten");
}

#[test]
fn abfrage_fuer_ein_projekt() {
    assert_eq!(
        jql_fuer_projekt("BAU").unwrap(),
        "project = BAU AND statusCategory != Done ORDER BY updated DESC"
    );
    for kaputt in ["", "1AB", "A) OR (B", "BAU-1"] {
        assert!(
            matches!(jql_fuer_projekt(kaputt), Err(Fehler::Einstellung(_))),
            "{kaputt}"
        );
    }
}

#[test]
fn debug_zeigt_den_schluessel_nicht() {
    let c = JiraClient::new(
        "https://firma.atlassian.net",
        "a@b.de",
        Secret::new("geheim-123"),
    )
    .unwrap();
    assert!(!format!("{c:?}").contains("geheim-123"));
}
