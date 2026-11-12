//! GitHub: Abbildung und Client gegen nachgebaute Antworten (`tests/fixtures`).
//! Kein Netzwerkzugriff; der Client spricht mit einem lokalen Mock-Server.

use cockpit_core::{Checks, LineKind, ReasonKind, Scope, Source, TicketStatus};
use cockpit_integrations::github::{
    GitHubClient, Relation, aggregate_checks, comments_to_core, dto::*, file_to_diff, pull_to_item,
    pull_to_review,
};
use cockpit_integrations::{Error, Secret, time};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn json<T: serde::de::DeserializeOwned>(s: &str) -> T {
    serde_json::from_str(s).expect("Fixture ist gültiges JSON")
}

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("fixtures/", $name))
    };
}

/// 2024-05-14T12:00:00Z
fn now() -> i64 {
    time::parse_iso("2024-05-14T12:00:00Z").unwrap()
}

#[test]
fn checks_become_condensed() {
    let failed: CheckRuns = json(fixture!("github_check_runs_failed.json"));
    let ok: CheckRuns = json(fixture!("github_check_runs_ok.json"));
    assert_eq!(aggregate_checks(&failed.check_runs), Checks::Failed);
    assert_eq!(aggregate_checks(&ok.check_runs), Checks::Passed);
    assert_eq!(aggregate_checks(&[]), Checks::Passed);

    let running = vec![
        CheckRun {
            name: "a".into(),
            status: "completed".into(),
            conclusion: Some("success".into()),
            ..CheckRun::default()
        },
        CheckRun {
            name: "b".into(),
            status: "queued".into(),
            conclusion: None,
            ..CheckRun::default()
        },
    ];
    assert_eq!(aggregate_checks(&running), Checks::Running);
}

#[test]
fn pull_becomes_an_item_with_review_request() {
    let pull: Pull = json(fixture!("github_pull_87.json"));
    let item = pull_to_item(
        "acme/baukasten",
        &pull,
        Checks::Passed,
        Relation::ReviewRequested,
        now(),
    );
    assert_eq!(item.id, "gh-acme-baukasten-87");
    assert_eq!(item.key, "#87");
    assert_eq!(item.source, Source::GitHub);
    assert_eq!(item.reason, ReasonKind::ReviewRequested);
    assert_eq!(item.scope, Scope::Mine);
    assert_eq!(item.status, TicketStatus::Review);
    assert_eq!(item.owner.name, "tobias-lang");
    assert_eq!(item.owner.initials, "TL");
    assert_eq!(item.time, "vor 2 Std.");
    assert_eq!(
        item.branch.as_deref(),
        Some("feature/BAU-142-avatar-tokens")
    );
    assert!(item.description.contains("Closes BAU-142"));
    assert_eq!(item.repo.as_deref(), Some("acme/baukasten"));
    assert_eq!(item.labels, ["design", "bug"]);
    let pr = item.pr.expect("PrRef gesetzt");
    assert_eq!(
        (pr.number, pr.draft, pr.checks),
        (87, false, Checks::Passed)
    );
}

#[test]
fn own_draft_without_description() {
    let pull: Pull = json(fixture!("github_pull_91.json"));
    let red = pull_to_item(
        "acme/baukasten",
        &pull,
        Checks::Failed,
        Relation::Own,
        now(),
    );
    assert_eq!(red.reason, ReasonKind::ChecksFailed);
    assert_eq!(red.description, "");
    assert!(red.labels.is_empty(), "Pull Request ohne Labels");
    assert!(red.pr.as_ref().unwrap().draft);
    let green = pull_to_item(
        "acme/baukasten",
        &pull,
        Checks::Passed,
        Relation::Own,
        now(),
    );
    assert_eq!(green.reason, ReasonKind::Comment);
}

#[test]
fn review_with_files_and_ticket() {
    let pull: Pull = json(fixture!("github_pull_87.json"));
    let files: Vec<PullFile> = json(fixture!("github_files_87.json"));
    let review = pull_to_review(&pull, &files, Checks::Failed);

    assert_eq!(review.number, 87);
    assert_eq!(review.base, "main");
    assert_eq!(review.branch, "feature/BAU-142-avatar-tokens");
    assert_eq!(review.ticket_key.as_deref(), Some("BAU-142"));
    assert_eq!(review.checks, Checks::Failed);
    assert_eq!(review.files.len(), 3);
    assert!(review.suggestions.is_empty() && !review.sent);

    let css = &review.files[0];
    assert_eq!(css.name, "features/ui/components.css");
    assert_eq!(css.stat(), "+2 −2");
    // Entfernte Zeile zählt im alten, neue im neuen Stand.
    let del = css.lines.iter().find(|l| l.kind == LineKind::Del).unwrap();
    assert_eq!((del.no, del.text.as_str()), (413, "  width: 28px;"));
    let add = css.lines.iter().find(|l| l.kind == LineKind::Add).unwrap();
    assert_eq!(add.no, 413);

    assert_eq!(review.files[1].lines.len(), 1);
    // Binärdatei ohne Patch: Datei bleibt, Zeilen sind leer.
    assert_eq!(file_to_diff(&files[2]).lines.len(), 0);
}

#[test]
fn ticket_falls_back_to_the_title() {
    let mut pull: Pull = json(fixture!("github_pull_87.json"));
    pull.head.name = "feature/avatar".into();
    assert_eq!(
        pull_to_review(&pull, &[], Checks::Passed)
            .ticket_key
            .as_deref(),
        Some("BAU-142")
    );
    pull.title = "Ohne Ticket".into();
    assert_eq!(pull_to_review(&pull, &[], Checks::Passed).ticket_key, None);
}

#[test]
fn comments_sorted_with_own_and_line_reference() {
    let issue: Vec<IssueComment> = json(fixture!("github_issue_comments_87.json"));
    let review: Vec<ReviewComment> = json(fixture!("github_review_comments_87.json"));
    let c = comments_to_core(&issue, &review, "mkeller", now());

    assert_eq!(c.len(), 4);
    // älteste zuerst: der veraltete Zeilenkommentar vom 13.5.
    assert_eq!(c[0].text, "features/ui/components.css\nVeralteter Hinweis");
    assert_eq!(c[1].text, "Bitte noch das Dark-Theme prüfen.");
    assert!(c[1].own);
    assert_eq!(c[1].author, cockpit_core::Person::me());
    assert_eq!(
        c[2].text,
        "styles/tokens.css · Zeile 1\nWarum 28px und kein Token aus der Skala?"
    );
    assert_eq!(c[3].author.name, "tobias-lang");
    assert!(!c[3].own);
    assert_eq!(c[3].time, "vor 5 Min.");
}

// ---------- Client gegen Mock-Server ----------

async fn client(server: &MockServer) -> GitHubClient {
    GitHubClient::new(&server.uri(), Secret::new("ghp_testtoken")).unwrap()
}

fn response(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "application/json")
}

#[tokio::test]
async fn inbox_load_without_duplicates() {
    let server = MockServer::start().await;
    // Der Schlüssel muss als Bearer-Header ankommen.
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            "is:pr is:open archived:false review-requested:@me",
        ))
        .and(header("authorization", "Bearer ghp_testtoken"))
        .respond_with(response(fixture!("github_search_review.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param("q", "is:pr is:open archived:false author:@me"))
        .respond_with(response(fixture!("github_search_own.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/baukasten/pulls/87"))
        .respond_with(response(fixture!("github_pull_87.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/baukasten/pulls/91"))
        .respond_with(response(fixture!("github_pull_91.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/baukasten/commits/aaa111/check-runs"))
        .respond_with(response(fixture!("github_check_runs_ok.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/baukasten/commits/ccc333/check-runs"))
        .respond_with(response(fixture!("github_check_runs_failed.json")))
        .mount(&server)
        .await;

    let items = client(&server).await.load_inbox(now()).await.unwrap();
    // #87 steht in beiden Suchen, kommt aber nur einmal vor – als Review-Anfrage.
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].key, "#87");
    assert_eq!(items[0].reason, ReasonKind::ReviewRequested);
    assert_eq!(items[1].key, "#91");
    assert_eq!(items[1].reason, ReasonKind::ChecksFailed);
}

#[tokio::test]
async fn files_comments_and_me() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(response(fixture!("github_user.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/baukasten/pulls/87/files"))
        .and(query_param("page", "1"))
        .respond_with(response(fixture!("github_files_87.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/baukasten/issues/87/comments"))
        .respond_with(response(fixture!("github_issue_comments_87.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/baukasten/pulls/87/comments"))
        .respond_with(response(fixture!("github_review_comments_87.json")))
        .mount(&server)
        .await;

    let c = client(&server).await;
    assert_eq!(c.me().await.unwrap(), "mkeller");
    assert_eq!(c.files("acme/baukasten", 87).await.unwrap().len(), 3);
    assert_eq!(c.comments("acme/baukasten", 87).await.unwrap().len(), 2);
    assert_eq!(
        c.line_comments("acme/baukasten", 87)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn status_codes_become_errors() {
    let server = MockServer::start().await;
    let mock_get = |route: &'static str, response: ResponseTemplate| {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response)
    };
    mock_get("/repos/a/b/pulls/1", ResponseTemplate::new(401))
        .mount(&server)
        .await;
    mock_get("/repos/a/b/pulls/2", ResponseTemplate::new(404))
        .mount(&server)
        .await;
    mock_get("/repos/a/b/pulls/3", ResponseTemplate::new(403))
        .mount(&server)
        .await;
    mock_get(
        "/repos/a/b/pulls/4",
        ResponseTemplate::new(403).insert_header("x-ratelimit-remaining", "0"),
    )
    .mount(&server)
    .await;
    mock_get("/repos/a/b/pulls/5", ResponseTemplate::new(500))
        .mount(&server)
        .await;
    mock_get("/repos/a/b/pulls/6", response("{ kaputt"))
        .mount(&server)
        .await;

    let c = client(&server).await;
    assert_eq!(c.pull("a/b", 1).await.unwrap_err(), Error::NotSignedIn);
    assert_eq!(c.pull("a/b", 2).await.unwrap_err(), Error::NotFound);
    assert_eq!(
        c.pull("a/b", 3).await.unwrap_err(),
        Error::NoPermission
    );
    assert_eq!(
        c.pull("a/b", 4).await.unwrap_err(),
        Error::RateLimit { wait_s: None }
    );
    assert_eq!(c.pull("a/b", 5).await.unwrap_err(), Error::Status(500));
    assert!(matches!(
        c.pull("a/b", 6).await.unwrap_err(),
        Error::Response(_)
    ));
}

#[tokio::test]
async fn unreachable_is_a_network_error_without_key() {
    // Port, auf dem niemand lauscht.
    let c = GitHubClient::new("http://127.0.0.1:9", Secret::new("ghp_geheim")).unwrap();
    let error = c.me().await.unwrap_err();
    assert!(matches!(error, Error::Network(_)));
    assert!(!error.to_string().contains("ghp_geheim"));
    assert!(!format!("{c:?}").contains("ghp_geheim"));
}

#[test]
fn insecure_address_is_rejected() {
    assert!(matches!(
        GitHubClient::new("http://api.github.com", Secret::new("x")),
        Err(Error::Config(_))
    ));
}
