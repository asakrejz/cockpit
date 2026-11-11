//! Schreibaufrufe gegen einen Mock-Server: richtige Methode, Pfad, Body und Auth-Header;
//! Fehlerfälle; der Schlüssel taucht nie in Meldungen auf. Nichts davon ist gegen die echten
//! Dienste geprüft.

use cockpit_core::{MergeMethod, TicketStatus};
use cockpit_integrations::ausgehend::{Ausgehend, ReviewDaten, ZeilenKommentar};
use cockpit_integrations::github::GitHubClient;
use cockpit_integrations::jira::JiraClient;
use cockpit_integrations::{Dienste, Fehler, GitHubDienst, JiraDienst, Secret};
use serde_json::json;
use wiremock::matchers::{body_json, header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "ghp_sehr_geheim_123";

async fn github(server: &MockServer) -> GitHubClient {
    GitHubClient::new(&server.uri(), Secret::new(TOKEN)).unwrap()
}

async fn jira(server: &MockServer) -> JiraClient {
    JiraClient::new(&server.uri(), "ich@firma.example", Secret::new(TOKEN)).unwrap()
}

fn bearer() -> impl wiremock::Match {
    header("authorization", format!("Bearer {TOKEN}").as_str())
}

fn review() -> ReviewDaten {
    ReviewDaten {
        urteil: "REQUEST_CHANGES",
        text: "Bitte nachbessern".into(),
        kommentare: vec![
            ZeilenKommentar {
                pfad: "a.rs".into(),
                zeile: 12,
                seite: "RIGHT",
                text: "Besser so\n\n```suggestion\n    neu();\n```".into(),
            },
            ZeilenKommentar {
                pfad: "a.rs".into(),
                zeile: 3,
                seite: "LEFT",
                text: "weg".into(),
            },
        ],
        sha: "abc123".into(),
    }
}

// ---------- GitHub ----------

#[tokio::test]
async fn github_kommentar() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/issues/7/comments"))
        .and(bearer())
        .and(header("x-github-api-version", "2022-11-28"))
        .and(body_json(json!({"body": "Hallo"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 1})))
        .expect(1)
        .mount(&s)
        .await;
    github(&s)
        .await
        .ausfuehren(&Ausgehend::GitHubKommentar {
            repo: "a/x".into(),
            nummer: 7,
            text: "Hallo".into(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn github_review_mit_positionskommentaren_und_suggestion() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/pulls/7/reviews"))
        .and(bearer())
        .and(body_json(json!({
            "event": "REQUEST_CHANGES",
            "body": "Bitte nachbessern",
            "commit_id": "abc123",
            "comments": [
                {"path": "a.rs", "line": 12, "side": "RIGHT",
                 "body": "Besser so\n\n```suggestion\n    neu();\n```"},
                {"path": "a.rs", "line": 3, "side": "LEFT", "body": "weg"}
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 9})))
        .expect(1)
        .mount(&s)
        .await;
    github(&s)
        .await
        .ausfuehren(&Ausgehend::GitHubReview {
            repo: "a/x".into(),
            nummer: 7,
            review: review(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn github_merge_mit_methode() {
    let s = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/repos/a/x/pulls/7/merge"))
        .and(bearer())
        .and(body_json(
            json!({"merge_method": "squash", "sha": "abc123"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"merged": true})))
        .expect(1)
        .mount(&s)
        .await;
    github(&s)
        .await
        .ausfuehren(&Ausgehend::GitHubMerge {
            repo: "a/x".into(),
            nummer: 7,
            methode: MergeMethod::Squash,
            sha: "abc123".into(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn github_branch_vom_standard_branch() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x"))
        .and(bearer())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"default_branch": "trunk"})))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/git/ref/heads/trunk"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"object": {"sha": "deadbeef"}})),
        )
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/git/refs"))
        .and(bearer())
        .and(body_json(
            json!({"ref": "refs/heads/feature/ABC-1-x", "sha": "deadbeef"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .expect(1)
        .mount(&s)
        .await;
    github(&s)
        .await
        .ausfuehren(&Ausgehend::GitHubBranch {
            repo: "a/x".into(),
            name: "feature/ABC-1-x".into(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn github_check_neu_anfordern() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/pulls/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "number": 7, "title": "t", "user": {"login": "x"},
            "head": {"ref": "f", "sha": "cafe"}, "base": {"ref": "main"}
        })))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/a/x/commits/cafe/check-runs"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"check_runs": [
                {"id": 11, "name": "Lint", "status": "completed", "conclusion": "success"},
                {"id": 22, "name": "Tests", "status": "completed", "conclusion": "failure"}
            ]})),
        )
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/check-runs/22/rerequest"))
        .and(bearer())
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&s)
        .await;
    let c = github(&s).await;
    c.ausfuehren(&Ausgehend::GitHubCheckNeu {
        repo: "a/x".into(),
        nummer: 7,
        check: "Tests".into(),
    })
    .await
    .unwrap();
    // Einen Check, den es nicht gibt, meldet der Client verständlich.
    let fehler = c
        .check_neu_starten("a/x", 7, "Gibt es nicht")
        .await
        .unwrap_err();
    assert!(fehler.to_string().contains("Gibt es nicht"), "{fehler}");
}

// ---------- Jira ----------

#[tokio::test]
async fn jira_kommentar_als_adf() {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .and(header_exists("authorization"))
        .and(body_json(
            json!({"body": {"type": "doc", "version": 1, "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Eins"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "Zwei"}]}
            ]}}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": "1"})))
        .expect(1)
        .mount(&s)
        .await;
    jira(&s)
        .await
        .ausfuehren(&Ausgehend::JiraKommentar {
            key: "ABC-1".into(),
            text: "Eins\nZwei".into(),
        })
        .await
        .unwrap();
    // Basic-Auth mit E-Mail und Token.
    let anfragen = s.received_requests().await.unwrap();
    let auth = anfragen[0]
        .headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(auth.starts_with("Basic "), "{auth}");
}

fn transitionen() -> serde_json::Value {
    json!({"transitions": [
        {"id": "11", "name": "Start", "to": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}}},
        {"id": "31", "name": "Erledigt", "to": {"name": "Done", "statusCategory": {"key": "done"}}}
    ]})
}

#[tokio::test]
async fn jira_status_sucht_die_passende_transition() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/transitions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(transitionen()))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue/ABC-1/transitions"))
        .and(body_json(json!({"transition": {"id": "31"}})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&s)
        .await;
    jira(&s)
        .await
        .ausfuehren(&Ausgehend::JiraStatus {
            key: "ABC-1".into(),
            status: TicketStatus::Done,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn jira_ohne_passende_transition_ruft_nichts_auf() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/transitions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(transitionen()))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&s)
        .await;
    let fehler = jira(&s)
        .await
        .status_setzen("ABC-1", TicketStatus::Review)
        .await
        .unwrap_err();
    assert_eq!(
        fehler.to_string(),
        "Jira bietet für ABC-1 keine Transition nach „Review“ an"
    );
}

// ---------- Fehlerfälle ----------

async fn kommentar_mit(antwort: ResponseTemplate) -> Fehler {
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(antwort)
        .mount(&s)
        .await;
    github(&s)
        .await
        .ausfuehren(&Ausgehend::GitHubKommentar {
            repo: "a/x".into(),
            nummer: 7,
            text: "x".into(),
        })
        .await
        .unwrap_err()
}

#[tokio::test]
async fn fehler_401_403_404_422_und_rate_limit() {
    assert_eq!(
        kommentar_mit(ResponseTemplate::new(401)).await,
        Fehler::NichtAngemeldet
    );
    assert_eq!(
        kommentar_mit(ResponseTemplate::new(403)).await,
        Fehler::KeineBerechtigung
    );
    assert_eq!(
        kommentar_mit(ResponseTemplate::new(404)).await,
        Fehler::NichtGefunden
    );
    assert_eq!(
        kommentar_mit(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("retry-after", "30")
        )
        .await,
        Fehler::RateLimit { warten_s: Some(30) }
    );
    assert_eq!(
        kommentar_mit(
            ResponseTemplate::new(422)
                .set_body_json(json!({"message": "Validation Failed", "errors": ["Zeile fehlt"]}))
        )
        .await,
        Fehler::Abgelehnt("Validation Failed; Zeile fehlt".into())
    );
}

#[tokio::test]
async fn merge_wird_abgelehnt_mit_der_meldung_des_dienstes() {
    let s = MockServer::start().await;
    Mock::given(method("PUT"))
        .respond_with(
            ResponseTemplate::new(405)
                .set_body_json(json!({"message": "Pull Request is not mergeable"})),
        )
        .mount(&s)
        .await;
    let fehler = github(&s)
        .await
        .ausfuehren(&Ausgehend::GitHubMerge {
            repo: "a/x".into(),
            nummer: 7,
            methode: MergeMethod::Merge,
            sha: String::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(
        fehler.to_string(),
        "Der Dienst lehnt die Anfrage ab: Pull Request is not mergeable"
    );
}

// ---------- Schlüssel ----------

#[tokio::test]
async fn schluessel_steht_nie_in_fehlern_oder_debug() {
    // Ein Dienst, der den Schlüssel in der Meldung zurückspielt.
    let s = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({"message": format!("Bad credentials: {TOKEN}")})),
        )
        .mount(&s)
        .await;
    let c = github(&s).await;
    let fehler = c
        .ausfuehren(&Ausgehend::GitHubKommentar {
            repo: "a/x".into(),
            nummer: 1,
            text: "x".into(),
        })
        .await
        .unwrap_err();
    for text in [fehler.to_string(), format!("{fehler:?}"), format!("{c:?}")] {
        assert!(!text.contains(TOKEN), "{text}");
    }

    // Netz weg.
    let c = GitHubClient::new("http://127.0.0.1:9", Secret::new(TOKEN)).unwrap();
    let fehler = c
        .ausfuehren(&Ausgehend::GitHubKommentar {
            repo: "a/x".into(),
            nummer: 1,
            text: "x".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(fehler, Fehler::Netzwerk(_)));
    assert!(!fehler.to_string().contains(TOKEN));

    // Jira: Token steckt im Basic-Header, nie in Meldungen.
    let j = JiraClient::new("http://127.0.0.1:9", "a@b.de", Secret::new(TOKEN)).unwrap();
    assert!(!format!("{j:?}").contains(TOKEN));
}

#[tokio::test]
async fn dienste_leiten_an_den_richtigen_client() {
    let g = MockServer::start().await;
    let j = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/a/x/issues/7/comments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .expect(1)
        .mount(&g)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .expect(1)
        .mount(&j)
        .await;
    let dienste = Dienste {
        github: Some(GitHubDienst {
            client: github(&g).await,
            repos: vec!["a/x".into()],
        }),
        jira: Some(JiraDienst {
            client: jira(&j).await,
            jql: "x".into(),
        }),
    };
    assert!(dienste.ziele().github && dienste.ziele().jira);
    dienste
        .ausfuehren(&Ausgehend::GitHubKommentar {
            repo: "a/x".into(),
            nummer: 7,
            text: "a".into(),
        })
        .await
        .unwrap();
    dienste
        .ausfuehren(&Ausgehend::JiraKommentar {
            key: "ABC-1".into(),
            text: "b".into(),
        })
        .await
        .unwrap();
    // Ohne Jira: verständlicher Fehler statt Absturz.
    let nur_github = Dienste {
        jira: None,
        github: None,
    };
    assert!(nur_github.ist_leer());
    assert!(
        nur_github
            .ausfuehren(&Ausgehend::JiraKommentar {
                key: "ABC-1".into(),
                text: "b".into()
            })
            .await
            .is_err()
    );
}
