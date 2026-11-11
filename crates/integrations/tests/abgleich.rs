//! Abgleich gegen nachgebaute Antworten (Fixtures): Abruf über die Clients, Zusammenführen
//! in den Workspace, lokaler Zustand bleibt, Fehler werden zum Zustand der Quelle.

use cockpit_core::{
    Action, Actor, Decision, Outcome, Problem, Remote, Source, Verdict, Workspace, sync,
};
use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::jira::{JiraClient, jql_fuer};
use cockpit_integrations::{Dienste, GitHubDienst, JiraDienst, Secret};
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("fixtures/", $name))
    };
}

const REPO: &str = "acme/baukasten";
/// 2024-05-14T12:00:00Z in Millisekunden
const JETZT_MS: u64 = 1_715_688_000_000;

fn ok(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(text.to_string(), "application/json")
}

async fn get(s: &MockServer, p: &str, antwort: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(antwort)
        .mount(s)
        .await;
}

/// GitHub mit Fixtures: #87 (Review angefragt) und #91 (eigener PR, Checks rot).
async fn github_server() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            format!("is:pr is:open archived:false review-requested:@me repo:{REPO}"),
        ))
        .respond_with(ok(fixture!("github_search_review.json")))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            format!("is:pr is:open archived:false author:@me repo:{REPO}"),
        ))
        .respond_with(ok(fixture!("github_search_own.json")))
        .mount(&s)
        .await;
    let basis = format!("/repos/{REPO}");
    get(
        &s,
        &format!("{basis}/pulls/87"),
        ok(fixture!("github_pull_87.json")),
    )
    .await;
    get(
        &s,
        &format!("{basis}/pulls/91"),
        ok(fixture!("github_pull_91.json")),
    )
    .await;
    get(
        &s,
        &format!("{basis}/pulls/87/files"),
        ok(fixture!("github_files_87.json")),
    )
    .await;
    get(&s, &format!("{basis}/pulls/91/files"), ok("[]")).await;
    get(
        &s,
        &format!("{basis}/issues/87/comments"),
        ok(fixture!("github_issue_comments_87.json")),
    )
    .await;
    get(&s, &format!("{basis}/issues/91/comments"), ok("[]")).await;
    get(
        &s,
        &format!("{basis}/pulls/87/comments"),
        ok(fixture!("github_review_comments_87.json")),
    )
    .await;
    get(&s, &format!("{basis}/pulls/91/comments"), ok("[]")).await;
    get(
        &s,
        &format!("{basis}/pulls/91/reviews"),
        ok(r#"[{"user":{"login":"tobias-lang"},"state":"CHANGES_REQUESTED","body":"Bitte Fokus prüfen","submitted_at":"2024-05-14T11:00:00Z"}]"#),
    )
    .await;
    get(&s, &format!("{basis}/pulls/87/reviews"), ok("[]")).await;
    get(
        &s,
        &format!("{basis}/commits/aaa111/check-runs"),
        ok(fixture!("github_check_runs_ok.json")),
    )
    .await;
    get(
        &s,
        &format!("{basis}/commits/{}/check-runs", sha91()),
        ok(fixture!("github_check_runs_failed.json")),
    )
    .await;
    s
}

fn sha91() -> String {
    let v: serde_json::Value = serde_json::from_str(fixture!("github_pull_91.json")).unwrap();
    v["head"]["sha"].as_str().unwrap().to_string()
}

async fn jira_server() -> MockServer {
    let s = MockServer::start().await;
    get(&s, "/rest/api/3/myself", ok(fixture!("jira_myself.json"))).await;
    get(
        &s,
        "/rest/api/3/search/jql",
        ok(fixture!("jira_search.json")),
    )
    .await;
    get(
        &s,
        "/rest/api/3/issue/BAU-142/comment",
        ok(fixture!("jira_comments_142.json")),
    )
    .await;
    let andere: serde_json::Value = serde_json::from_str(fixture!("jira_search.json")).unwrap();
    for i in andere["issues"].as_array().unwrap() {
        let key = i["key"].as_str().unwrap();
        if key != "BAU-142" {
            get(
                &s,
                &format!("/rest/api/3/issue/{key}/comment"),
                ok(r#"{"comments":[]}"#),
            )
            .await;
        }
    }
    s
}

fn dienste(g: Option<&MockServer>, j: Option<&MockServer>) -> Dienste {
    Dienste {
        github: g.map(|s| GitHubDienst {
            client: GitHubClient::new(&s.uri(), Secret::new("ghp_x")).unwrap(),
            repos: vec![REPO.into()],
        }),
        jira: j.map(|s| JiraDienst {
            client: JiraClient::new(&s.uri(), "mira@firma.example", Secret::new("jt")).unwrap(),
            jql: jql_fuer(&[], None).unwrap(),
        }),
    }
}

async fn abgleichen(d: &Dienste, ws: &Workspace, ms: u64) -> (Workspace, Remote) {
    let remote = d.holen(ws, ms, &[]).await;
    let mut neu = ws.clone();
    neu.apply(
        Action::SyncRemote {
            remote: remote.clone(),
        },
        Actor::System,
        ms,
    )
    .unwrap();
    (neu, remote)
}

#[tokio::test]
async fn holt_pull_requests_tickets_kommentare_checks_und_diffs() {
    let g = github_server().await;
    let j = jira_server().await;
    let d = dienste(Some(&g), Some(&j));
    let (ws, _) = abgleichen(&d, &cockpit_core::demo::workspace(), JETZT_MS).await;

    // Beispieldaten sind weg, echte Einträge da.
    assert!(ws.item("bau142").is_none() && ws.item("pr87").is_none());
    let pr87 = ws.item("gh-acme-baukasten-87").expect("PR 87");
    assert_eq!(pr87.pr.as_ref().unwrap().number, 87);
    assert_eq!(pr87.comments.len(), 4, "Gespräch und Zeilenkommentare");
    let pr91 = ws.item("gh-acme-baukasten-91").expect("PR 91");
    assert!(
        pr91.pr.as_ref().unwrap().changes_requested,
        "Urteil von mkeller"
    );
    assert!(
        pr91.comments
            .iter()
            .any(|c| c.text.starts_with("Review: Änderungen angefordert")),
    );
    assert_eq!(
        pr91.pr.as_ref().unwrap().checks,
        cockpit_core::Checks::Failed
    );
    let jira = ws.item("bau-142").expect("BAU-142");
    assert_eq!(jira.source, Source::Jira);
    assert!(!jira.comments.is_empty());

    // Diff und einzelne Checks im Review, Zuordnung Nummer → Repo.
    let review = ws.review(87).unwrap();
    assert!(!review.files.is_empty());
    assert!(!review.check_runs.is_empty());
    assert_eq!(ws.review(91).unwrap().check_runs.len(), 3);
    assert_eq!(ws.sync.pr(87).unwrap().repo, REPO);
    assert_eq!(ws.sync.pr(87).unwrap().head_sha, "aaa111");
    assert!(ws.sync.state_of(Source::GitHub).unwrap().is_ok());
    assert!(ws.sync.state_of(Source::Jira).unwrap().is_ok());
    // Das Protokoll hält das Neue mit Urheber System fest.
    assert!(ws.log.iter().all(|e| e.actor == Actor::System));
    assert!(!ws.log.is_empty());
}

#[tokio::test]
async fn lokaler_zustand_bleibt_beim_naechsten_abgleich() {
    let g = github_server().await;
    let j = jira_server().await;
    let d = dienste(Some(&g), Some(&j));
    let (mut ws, _) = abgleichen(&d, &Workspace::default(), JETZT_MS).await;

    // Lokal: erledigt, zurückgestellt, Review-Entwurf, Urteil, Agent-Lauf.
    ws.mark_done("bau-142");
    ws.snooze("gh-acme-baukasten-91", None);
    let line = ws.review(87).unwrap().files[0]
        .lines
        .iter()
        .position(|l| l.kind == cockpit_core::LineKind::Add)
        .unwrap();
    let file = ws.review(87).unwrap().files[0].name.clone();
    ws.add_line_comment(87, &file, line, "Eigener Entwurf", Some("neu"))
        .unwrap();
    ws.set_verdict(87, Verdict::Approve);
    ws.reviews
        .iter_mut()
        .find(|r| r.number == 87)
        .unwrap()
        .suggestions
        .push(cockpit_core::Suggestion::new(
            "s1",
            "x",
            cockpit_core::Level::Note,
            "Hm",
        ));
    ws.decide(87, "s1", Some(Decision::Declined));
    ws.start_agent("BAU-142", cockpit_core::AgentOptions::default())
        .unwrap();

    let (nach, _) = abgleichen(&d, &ws, JETZT_MS + 120_000).await;
    assert!(nach.item("bau-142").unwrap().read);
    assert!(nach.snooze_of("gh-acme-baukasten-91").is_some());
    let r = nach.review(87).unwrap();
    assert_eq!(r.line_comments.len(), 1);
    assert_eq!(r.line_comments[0].text, "Eigener Entwurf");
    assert_eq!(r.verdict, Verdict::Approve);
    assert_eq!(r.suggestions[0].decision, Some(Decision::Declined));
    assert_eq!(nach.runs.len(), 1);
}

#[tokio::test]
async fn unveraendertes_pr_laedt_den_diff_nicht_noch_einmal() {
    let g = github_server().await;
    let d = dienste(Some(&g), None);
    let (ws, _) = abgleichen(&d, &Workspace::default(), JETZT_MS).await;
    let vorher = g
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/pulls/87/files"))
        .count();
    let (_, remote) = abgleichen(&d, &ws, JETZT_MS + 1).await;
    let nachher = g
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/pulls/87/files"))
        .count();
    assert_eq!(
        (vorher, nachher),
        (1, 1),
        "gleicher Commit: kein neuer Abruf"
    );
    // Und der Diff steht trotzdem im Remote.
    let Outcome::Loaded(l) = &remote.sources[0].outcome else {
        panic!("geladen erwartet")
    };
    assert!(
        !l.reviews
            .iter()
            .find(|r| r.number == 87)
            .unwrap()
            .files
            .is_empty()
    );
}

#[tokio::test]
async fn entferntes_verschwindet() {
    let g = github_server().await;
    let d = dienste(Some(&g), None);
    let (ws, _) = abgleichen(&d, &Workspace::default(), JETZT_MS).await;
    assert!(ws.item("gh-acme-baukasten-91").is_some());
    // Beim Dienst ist nur noch #87 übrig.
    g.reset().await;
    let g2 = github_server().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            format!("is:pr is:open archived:false author:@me repo:{REPO}"),
        ))
        .respond_with(ok(r#"{"items":[]}"#))
        .with_priority(1)
        .mount(&g2)
        .await;
    let d2 = dienste(Some(&g2), None);
    let (nach, _) = abgleichen(&d2, &ws, JETZT_MS + 1).await;
    assert!(nach.item("gh-acme-baukasten-91").is_none());
    assert!(nach.review(91).is_none() && nach.sync.pr(91).is_none());
    assert!(nach.item("gh-acme-baukasten-87").is_some());
}

#[tokio::test]
async fn fehler_werden_zum_zustand_der_quelle_und_nichts_geht_verloren() {
    let g = github_server().await;
    let d = dienste(Some(&g), None);
    let (ws, _) = abgleichen(&d, &Workspace::default(), JETZT_MS).await;

    let kaputt = |antwort: ResponseTemplate| async move {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(antwort)
            .mount(&s)
            .await;
        s
    };
    // 401: Token abgelaufen.
    let s = kaputt(ResponseTemplate::new(401)).await;
    let (nach, _) = abgleichen(&dienste(Some(&s), None), &ws, JETZT_MS + 1).await;
    assert_eq!(
        nach.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::Unauthorized)
    );
    assert!(nach.item("gh-acme-baukasten-87").is_some(), "Stand bleibt");

    // Kontingent: Pause bis Reset.
    let s = kaputt(
        ResponseTemplate::new(403)
            .insert_header("x-ratelimit-remaining", "0")
            .insert_header("retry-after", "90"),
    )
    .await;
    let (nach, _) = abgleichen(&dienste(Some(&s), None), &ws, JETZT_MS + 2).await;
    assert_eq!(
        nach.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::RateLimited {
            until_ms: Some(JETZT_MS + 2 + 90_000)
        })
    );
    assert_eq!(
        nach.sync.paused_until(Source::GitHub, JETZT_MS + 3),
        Some(JETZT_MS + 2 + 90_000)
    );
    // Pausiert: die Quelle wird gar nicht erst abgefragt.
    let leer = dienste(Some(&s), None)
        .holen(&nach, JETZT_MS + 3, &[Source::GitHub])
        .await;
    assert!(leer.sources.is_empty());

    // Netz weg.
    let weg = Dienste {
        github: Some(GitHubDienst {
            client: GitHubClient::new("http://127.0.0.1:9", Secret::new("x")).unwrap(),
            repos: vec![REPO.into()],
        }),
        jira: None,
    };
    let (nach, _) = abgleichen(&weg, &ws, JETZT_MS + 4).await;
    assert_eq!(
        nach.sync.state_of(Source::GitHub).unwrap().problem,
        Some(Problem::Offline)
    );
    assert!(nach.item("gh-acme-baukasten-87").is_some());
}

#[tokio::test]
async fn unbekanntes_repo_ist_ein_hinweis_kein_ausfall() {
    let g = github_server().await;
    let d = Dienste {
        github: Some(GitHubDienst {
            client: GitHubClient::new(&g.uri(), Secret::new("x")).unwrap(),
            repos: vec!["gibt/es-nicht".into(), REPO.into()],
        }),
        jira: None,
    };
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            "is:pr is:open archived:false review-requested:@me repo:gibt/es-nicht",
        ))
        .respond_with(
            ResponseTemplate::new(422).set_body_json(json!({"message": "Validation Failed"})),
        )
        .mount(&g)
        .await;
    let (ws, _) = abgleichen(&d, &Workspace::default(), JETZT_MS).await;
    let st = ws.sync.state_of(Source::GitHub).unwrap();
    assert!(st.is_ok());
    assert!(
        st.warnings[0].contains("gibt/es-nicht"),
        "{:?}",
        st.warnings
    );
    assert!(ws.item("gh-acme-baukasten-87").is_some());
}

#[tokio::test]
async fn jira_wird_seitenweise_gelesen() {
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
    let (ws, _) = abgleichen(&dienste(None, Some(&s)), &Workspace::default(), JETZT_MS).await;
    let keys: Vec<&str> = ws.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
}

#[tokio::test]
async fn github_kommentare_seitenweise() {
    // 100 Kommentare auf Seite 1, einer auf Seite 2.
    let g = github_server().await;
    let eintrag = |i: u32| json!({"user": {"login": "mkeller"}, "body": format!("k{i}"), "created_at": "2024-05-14T09:00:00Z"});
    let seite1: Vec<_> = (0..100).map(eintrag).collect();
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/issues/91/comments")))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([eintrag(100)])))
        .with_priority(1)
        .mount(&g)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/{REPO}/issues/91/comments")))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(seite1))
        .with_priority(1)
        .mount(&g)
        .await;
    let (ws, _) = abgleichen(&dienste(Some(&g), None), &Workspace::default(), JETZT_MS).await;
    assert_eq!(
        ws.item("gh-acme-baukasten-91").unwrap().comments.len(),
        101 + 1
    );
}

#[test]
fn jql_aus_projekten_und_eigener_abfrage() {
    assert_eq!(
        jql_fuer(&["BAU".into(), "WEB".into()], None).unwrap(),
        "project in (BAU, WEB) AND (assignee = currentUser() OR watcher = currentUser()) AND statusCategory != Done ORDER BY updated DESC"
    );
    assert_eq!(
        jql_fuer(&["BAU".into()], Some(" project = X ")).unwrap(),
        "project = X"
    );
    assert!(jql_fuer(&["BAU) OR (x".into()], None).is_err());
    assert!(jql_fuer(&[], None).unwrap().contains("currentUser()"));
}

#[test]
fn merge_bleibt_rein_und_deterministisch() {
    let a = sync::merge(&Workspace::default(), &Remote::default(), 1);
    let b = sync::merge(&Workspace::default(), &Remote::default(), 1);
    assert_eq!(a, b);
}
