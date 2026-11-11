//! Aktionen, die nach außen wirken: Welche [`Action`] geht an welchen Dienst, und wie sieht
//! der HTTP-Aufruf aus? Alles hier ist rein (kein Netz, keine Uhr) und wird ohne Server getestet.
//!
//! * [`plane`] sagt zu einer Aktion, was beim Dienst zu tun ist ([`Ausgehend`]) – oder `None`,
//!   wenn die Aktion lokal bleibt (andere Aktionen, Einträge aus den Beispieldaten, Quelle aus).
//! * [`Ausgehend::anfrage`] baut daraus Methode, Pfad und Body, soweit ein Aufruf genügt.
//!   Mehrstufiges (Jira-Transition, Branch, Check neu starten) steht in den Clients; die
//!   reinen Teile davon (Auswahl, Bodies) stehen hier.
//!
//! Die Regel „nur Du darfst mergen“ prüft `Workspace::apply` und läuft **vor** dem Aufruf.

use cockpit_core::{
    Action, Decision, LineKind, MergeMethod, Review, TicketStatus, Verdict, Workspace,
};
use serde_json::{Value, json};

use crate::jira::dto::Transition;
use crate::jira::{status_von, text_to_adf};

/// Was der Server an Diensten hat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ziele {
    pub github: bool,
    pub jira: bool,
    /// Konfigurierte GitHub-Repos (`owner/name`); in das erste kommt ein Branch für ein Jira-Ticket.
    pub repos: Vec<String>,
}

/// Was beim Dienst zu tun ist.
#[derive(Clone, Debug, PartialEq)]
pub enum Ausgehend {
    JiraKommentar {
        key: String,
        text: String,
    },
    JiraStatus {
        key: String,
        status: TicketStatus,
    },
    GitHubKommentar {
        repo: String,
        nummer: u32,
        text: String,
    },
    GitHubBranch {
        repo: String,
        name: String,
    },
    GitHubReview {
        repo: String,
        nummer: u32,
        review: ReviewDaten,
    },
    GitHubMerge {
        repo: String,
        nummer: u32,
        methode: MergeMethod,
        /// Der Commit, den wir gesehen haben – ein neuer Push lässt das Mergen scheitern.
        sha: String,
    },
    GitHubCheckNeu {
        repo: String,
        nummer: u32,
        check: String,
    },
}

/// Ein Review, wie es an GitHub geht.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewDaten {
    /// `COMMENT`, `APPROVE` oder `REQUEST_CHANGES`
    pub urteil: &'static str,
    pub text: String,
    pub kommentare: Vec<ZeilenKommentar>,
    /// Der Commit, den wir geprüft haben.
    pub sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZeilenKommentar {
    pub pfad: String,
    pub zeile: u32,
    /// `RIGHT` (neue und unveränderte Zeilen) oder `LEFT` (entfernte).
    pub seite: &'static str,
    pub text: String,
}

/// Ein einzelner HTTP-Aufruf.
#[derive(Clone, Debug, PartialEq)]
pub struct Anfrage {
    pub methode: &'static str,
    pub pfad: String,
    pub body: Option<Value>,
}

/// Warum sich eine Aktion nicht nach außen abbilden lässt (deutsche Meldung).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanFehler(pub String);

impl std::fmt::Display for PlanFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Bestimmt den Aufruf zu einer Aktion. `vorher` ist der Stand vor, `nachher` der Stand nach
/// einer Probe-Anwendung (so kennen wir z. B. den Namen des neuen Branches). `Ok(None)`:
/// bleibt lokal.
pub fn plane(
    vorher: &Workspace,
    nachher: &Workspace,
    action: &Action,
    ziele: &Ziele,
) -> Result<Option<Ausgehend>, PlanFehler> {
    let repo_von_pr = |nummer: u32| -> Option<(String, String)> {
        vorher
            .sync
            .pr(nummer)
            .filter(|_| ziele.github)
            .map(|l| (l.repo.clone(), l.head_sha.clone()))
    };
    Ok(match action {
        Action::Reply { item, text } => {
            let Some(i) = vorher.item(item).filter(|i| vorher.sync.is_synced(&i.id)) else {
                return Ok(None);
            };
            let text = text.trim().to_string();
            if text.is_empty() {
                return Ok(None);
            }
            match (i.source, i.pr.as_ref()) {
                (cockpit_core::Source::Jira, _) if ziele.jira => Some(Ausgehend::JiraKommentar {
                    key: i.key.clone(),
                    text,
                }),
                (cockpit_core::Source::GitHub, Some(pr)) => {
                    repo_von_pr(pr.number).map(|(repo, _)| Ausgehend::GitHubKommentar {
                        repo,
                        nummer: pr.number,
                        text,
                    })
                }
                _ => None,
            }
        }
        Action::SetStatus { item, status } => {
            let Some(i) = vorher.item(item).filter(|i| vorher.sync.is_synced(&i.id)) else {
                return Ok(None);
            };
            (i.source == cockpit_core::Source::Jira && i.is_ticket() && ziele.jira).then(|| {
                Ausgehend::JiraStatus {
                    key: i.key.clone(),
                    status: *status,
                }
            })
        }
        Action::CreateBranch { item } => {
            let Some(i) = vorher.item(item).filter(|i| vorher.sync.is_synced(&i.id)) else {
                return Ok(None);
            };
            if !(i.source == cockpit_core::Source::Jira && ziele.github) {
                return Ok(None);
            }
            let Some(repo) = ziele.repos.first() else {
                return Err(PlanFehler(
                    "Für Branches ist kein GitHub-Repo konfiguriert".into(),
                ));
            };
            let name = nachher
                .item(item)
                .and_then(|n| n.branch.clone())
                .ok_or_else(|| PlanFehler("Der Branch-Name steht nicht fest".into()))?;
            Some(Ausgehend::GitHubBranch {
                repo: repo.clone(),
                name,
            })
        }
        Action::SendReview { review, comment } => {
            let (Some(r), Some((repo, sha))) = (vorher.review(*review), repo_von_pr(*review))
            else {
                return Ok(None);
            };
            Some(Ausgehend::GitHubReview {
                repo,
                nummer: *review,
                review: review_daten(r, comment, &sha),
            })
        }
        Action::MergePr { review, method } => {
            repo_von_pr(*review).map(|(repo, sha)| Ausgehend::GitHubMerge {
                repo,
                nummer: *review,
                methode: *method,
                sha,
            })
        }
        Action::RestartCheck { review, check } => {
            repo_von_pr(*review).map(|(repo, _)| Ausgehend::GitHubCheckNeu {
                repo,
                nummer: *review,
                check: check.clone(),
            })
        }
        _ => None,
    })
}

// ---------- Review ----------

fn urteil(v: Verdict) -> &'static str {
    match v {
        Verdict::Comment => "COMMENT",
        Verdict::Approve => "APPROVE",
        Verdict::RequestChanges => "REQUEST_CHANGES",
    }
}

/// `datei · Zeile 12` → `("datei", 12)`.
fn ort_lesen(ort: &str) -> Option<(&str, u32)> {
    let (datei, zeile) = ort.rsplit_once(" · Zeile ")?;
    Some((datei.trim(), zeile.trim().parse().ok()?))
}

/// Baut das Review aus dem Stand: Urteil, eigener Text, freigegebene Vorschläge des Agenten
/// und alle eigenen Zeilenkommentare (mit `suggestion`-Blöcken). Was sich keiner Zeile
/// zuordnen lässt, steht im Text des Reviews, damit nichts verloren geht.
pub fn review_daten(review: &Review, kommentar: &str, sha: &str) -> ReviewDaten {
    let mut text = kommentar.trim().to_string();
    let mut zeilen: Vec<ZeilenKommentar> = Vec::new();
    let mut lose: Vec<String> = Vec::new();

    for s in review
        .suggestions
        .iter()
        .filter(|s| s.decision == Some(Decision::Approved))
    {
        let zeile = ort_lesen(&s.location).filter(|(datei, nr)| {
            review.files.iter().any(|f| {
                f.name == *datei
                    && f.lines
                        .iter()
                        .any(|l| l.no == *nr && l.kind != LineKind::Del)
            })
        });
        match zeile {
            Some((datei, nr)) => zeilen.push(ZeilenKommentar {
                pfad: datei.into(),
                zeile: nr,
                seite: "RIGHT",
                text: s.text.clone(),
            }),
            None => lose.push(format!("{}: {}", s.location, s.text)),
        }
    }
    for c in &review.line_comments {
        let zeile = review
            .files
            .iter()
            .find(|f| f.name == c.file)
            .and_then(|f| f.lines.get(c.line));
        match zeile {
            Some(l) => zeilen.push(ZeilenKommentar {
                pfad: c.file.clone(),
                zeile: l.no,
                seite: if l.kind == LineKind::Del {
                    "LEFT"
                } else {
                    "RIGHT"
                },
                text: c.body(),
            }),
            None => lose.push(format!("{}: {}", c.file, c.body())),
        }
    }
    if !lose.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&lose.join("\n\n"));
    }
    // GitHub verlangt Text, wenn es keine Zeilenkommentare gibt (und bei „Änderungen“).
    if text.is_empty()
        && (review.verdict == Verdict::RequestChanges
            || (review.verdict == Verdict::Comment && zeilen.is_empty()))
    {
        text = match review.verdict {
            Verdict::RequestChanges => "Änderungen angefordert.".into(),
            _ => "Review aus dem Cockpit.".into(),
        };
    }
    ReviewDaten {
        urteil: urteil(review.verdict),
        text,
        kommentare: zeilen,
        sha: sha.into(),
    }
}

// ---------- Aufrufe ----------

impl ReviewDaten {
    fn body(&self) -> Value {
        let mut b = json!({ "event": self.urteil });
        if !self.text.is_empty() {
            b["body"] = json!(self.text);
        }
        if !self.sha.is_empty() {
            b["commit_id"] = json!(self.sha);
        }
        if !self.kommentare.is_empty() {
            b["comments"] = self
                .kommentare
                .iter()
                .map(|k| json!({"path": k.pfad, "line": k.zeile, "side": k.seite, "body": k.text}))
                .collect();
        }
        b
    }
}

impl Ausgehend {
    /// Name des Dienstes für Meldungen.
    pub fn dienst(&self) -> &'static str {
        match self {
            Ausgehend::JiraKommentar { .. } | Ausgehend::JiraStatus { .. } => "Jira",
            _ => "GitHub",
        }
    }

    /// Der eine Aufruf, soweit einer genügt. Mehrstufige Abläufe geben `None` zurück.
    pub fn anfrage(&self) -> Option<Anfrage> {
        Some(match self {
            Ausgehend::JiraKommentar { key, text } => Anfrage {
                methode: "POST",
                pfad: format!("/rest/api/3/issue/{key}/comment"),
                body: Some(json!({ "body": text_to_adf(text) })),
            },
            Ausgehend::GitHubKommentar { repo, nummer, text } => Anfrage {
                methode: "POST",
                pfad: format!("/repos/{repo}/issues/{nummer}/comments"),
                body: Some(json!({ "body": text })),
            },
            Ausgehend::GitHubReview {
                repo,
                nummer,
                review,
            } => Anfrage {
                methode: "POST",
                pfad: format!("/repos/{repo}/pulls/{nummer}/reviews"),
                body: Some(review.body()),
            },
            Ausgehend::GitHubMerge {
                repo,
                nummer,
                methode,
                sha,
            } => {
                let mut b = json!({ "merge_method": methode.id() });
                if !sha.is_empty() {
                    b["sha"] = json!(sha);
                }
                Anfrage {
                    methode: "PUT",
                    pfad: format!("/repos/{repo}/pulls/{nummer}/merge"),
                    body: Some(b),
                }
            }
            Ausgehend::JiraStatus { .. }
            | Ausgehend::GitHubBranch { .. }
            | Ausgehend::GitHubCheckNeu { .. } => return None,
        })
    }
}

/// Die Transition, die zum Ziel-Status führt: erst eine mit passendem Status, bei mehreren
/// die, deren Zielname dem deutschen oder englischen Namen des Status am nächsten kommt
/// (sonst die erste).
pub fn waehle_transition(liste: &[Transition], ziel: TicketStatus) -> Option<&Transition> {
    let passend: Vec<&Transition> = liste.iter().filter(|t| status_von(&t.to) == ziel).collect();
    passend.first().copied()
}

pub fn transition_anfrage(key: &str, id: &str) -> Anfrage {
    Anfrage {
        methode: "POST",
        pfad: format!("/rest/api/3/issue/{key}/transitions"),
        body: Some(json!({ "transition": { "id": id } })),
    }
}

pub fn branch_anfrage(repo: &str, name: &str, sha: &str) -> Anfrage {
    Anfrage {
        methode: "POST",
        pfad: format!("/repos/{repo}/git/refs"),
        body: Some(json!({ "ref": format!("refs/heads/{name}"), "sha": sha })),
    }
}

pub fn check_neu_anfrage(repo: &str, check_id: u64) -> Anfrage {
    Anfrage {
        methode: "POST",
        pfad: format!("/repos/{repo}/check-runs/{check_id}/rerequest"),
        body: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cockpit_core::{
        Actor, Checks, Comment, DiffLine, FileDiff, Item, Kind, Loaded, Outcome, Person, PrLink,
        PrRef, ReasonKind, Remote, Scope, Source, SourceData, Suggestion,
    };

    const T: u64 = 1_800_000_000_000;

    fn item(id: &str, key: &str, source: Source) -> Item {
        Item {
            id: id.into(),
            kind: if source == Source::Jira {
                Kind::Ticket
            } else {
                Kind::PullRequest
            },
            source,
            key: key.into(),
            title: "Avatar Größe".into(),
            description: String::new(),
            reason: ReasonKind::Assigned,
            reason_text: String::new(),
            time: String::new(),
            scope: Scope::Mine,
            owner: Person::me(),
            status: TicketStatus::Open,
            branch: None,
            pr: (source == Source::GitHub).then_some(PrRef {
                number: 7,
                draft: false,
                checks: Checks::Passed,
                changes_requested: false,
                merged: false,
            }),
            agent_note: None,
            comments: Vec::<Comment>::new(),
            read: false,
            due: None,
        }
    }

    fn review() -> Review {
        let mut r = cockpit_core::demo::workspace().reviews.remove(0);
        r.number = 7;
        r.files = vec![FileDiff {
            name: "a.rs".into(),
            lines: vec![
                DiffLine {
                    no: 1,
                    kind: LineKind::Context,
                    text: "fn a() {".into(),
                },
                DiffLine {
                    no: 2,
                    kind: LineKind::Del,
                    text: "    alt();".into(),
                },
                DiffLine {
                    no: 2,
                    kind: LineKind::Add,
                    text: "    neu();".into(),
                },
            ],
        }];
        r.suggestions = vec![
            Suggestion::new(
                "s1",
                "a.rs · Zeile 2",
                cockpit_core::Level::Note,
                "Name prüfen",
            ),
            Suggestion::new("s2", "unklar", cockpit_core::Level::Note, "Allgemein"),
            Suggestion::new(
                "s3",
                "a.rs · Zeile 1",
                cockpit_core::Level::Note,
                "abgelehnt",
            ),
        ];
        r.suggestions[0].decision = Some(Decision::Approved);
        r.suggestions[1].decision = Some(Decision::Approved);
        r.suggestions[2].decision = Some(Decision::Declined);
        r.line_comments.clear();
        r.verdict = Verdict::RequestChanges;
        r.add_line_comment("a.rs", 2, "Besser so", Some("    neuer();"))
            .unwrap();
        r.add_line_comment("a.rs", 0, "Warum?", None).unwrap();
        r
    }

    fn ws() -> Workspace {
        let remote = Remote {
            sources: vec![
                SourceData {
                    source: Source::Jira,
                    outcome: Outcome::Loaded(Loaded {
                        items: vec![item("abc-1", "ABC-1", Source::Jira)],
                        ..Loaded::default()
                    }),
                },
                SourceData {
                    source: Source::GitHub,
                    outcome: Outcome::Loaded(Loaded {
                        items: vec![item("gh-a-x-7", "#7", Source::GitHub)],
                        reviews: vec![review()],
                        prs: vec![PrLink {
                            number: 7,
                            repo: "a/x".into(),
                            head_sha: "abc123".into(),
                        }],
                        warnings: vec![],
                    }),
                },
            ],
        };
        cockpit_core::sync::merge(&Workspace::default(), &remote, T).0
    }

    fn alles() -> Ziele {
        Ziele {
            github: true,
            jira: true,
            repos: vec!["a/x".into()],
        }
    }

    fn plan(ws: &Workspace, a: Action, z: &Ziele) -> Result<Option<Ausgehend>, PlanFehler> {
        let mut nachher = ws.clone();
        let _ = nachher.apply(a.clone(), Actor::Du, T);
        plane(ws, &nachher, &a, z)
    }

    #[test]
    fn reply_geht_je_quelle_an_den_richtigen_dienst() {
        let w = ws();
        let a = plan(
            &w,
            Action::Reply {
                item: "abc-1".into(),
                text: " Hallo ".into(),
            },
            &alles(),
        );
        assert_eq!(
            a,
            Ok(Some(Ausgehend::JiraKommentar {
                key: "ABC-1".into(),
                text: "Hallo".into()
            }))
        );
        let a = plan(
            &w,
            Action::Reply {
                item: "gh-a-x-7".into(),
                text: "Hi".into(),
            },
            &alles(),
        );
        assert_eq!(
            a,
            Ok(Some(Ausgehend::GitHubKommentar {
                repo: "a/x".into(),
                nummer: 7,
                text: "Hi".into()
            }))
        );
        let anfrage = a.unwrap().unwrap().anfrage().unwrap();
        assert_eq!(
            (anfrage.methode, anfrage.pfad.as_str(), anfrage.body),
            (
                "POST",
                "/repos/a/x/issues/7/comments",
                Some(json!({"body": "Hi"}))
            )
        );
        let jira = plan(
            &w,
            Action::Reply {
                item: "abc-1".into(),
                text: "a\nb".into(),
            },
            &alles(),
        )
        .unwrap()
        .unwrap()
        .anfrage()
        .unwrap();
        assert_eq!(jira.pfad, "/rest/api/3/issue/ABC-1/comment");
        assert_eq!(
            jira.body.unwrap()["body"]["content"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn lokales_bleibt_lokal() {
        let w = ws();
        let demo = cockpit_core::demo::workspace();
        // Beispieldaten: nie nach außen.
        let a = plan(
            &demo,
            Action::Reply {
                item: "bau142".into(),
                text: "x".into(),
            },
            &alles(),
        );
        assert_eq!(a, Ok(None));
        // Quelle aus.
        let aus = Ziele::default();
        assert_eq!(
            plan(
                &w,
                Action::Reply {
                    item: "abc-1".into(),
                    text: "x".into()
                },
                &aus
            ),
            Ok(None)
        );
        // Andere Aktionen.
        assert_eq!(
            plan(
                &w,
                Action::MarkDone {
                    item: "abc-1".into()
                },
                &alles()
            ),
            Ok(None)
        );
        assert_eq!(plan(&w, Action::AdvanceAgents, &alles()), Ok(None));
        assert_eq!(
            plan(
                &w,
                Action::SetVerdict {
                    review: 7,
                    verdict: Verdict::Approve
                },
                &alles()
            ),
            Ok(None)
        );
    }

    #[test]
    fn status_branch_merge_und_check() {
        let w = ws();
        assert_eq!(
            plan(
                &w,
                Action::SetStatus {
                    item: "abc-1".into(),
                    status: TicketStatus::Done
                },
                &alles()
            ),
            Ok(Some(Ausgehend::JiraStatus {
                key: "ABC-1".into(),
                status: TicketStatus::Done
            }))
        );
        let Ok(Some(Ausgehend::GitHubBranch { repo, name })) = plan(
            &w,
            Action::CreateBranch {
                item: "abc-1".into(),
            },
            &alles(),
        ) else {
            panic!("Branch erwartet");
        };
        assert_eq!(repo, "a/x");
        assert_eq!(name, "feature/ABC-1-avatar-groesse");
        let ohne_repo = Ziele {
            repos: vec![],
            ..alles()
        };
        assert!(
            plan(
                &w,
                Action::CreateBranch {
                    item: "abc-1".into()
                },
                &ohne_repo
            )
            .is_err()
        );

        let m = plan(
            &w,
            Action::MergePr {
                review: 7,
                method: MergeMethod::Squash,
            },
            &alles(),
        );
        let anf = m.unwrap().unwrap().anfrage().unwrap();
        assert_eq!(
            (anf.methode, anf.pfad.as_str()),
            ("PUT", "/repos/a/x/pulls/7/merge")
        );
        assert_eq!(
            anf.body,
            Some(json!({"merge_method": "squash", "sha": "abc123"}))
        );

        assert_eq!(
            plan(
                &w,
                Action::RestartCheck {
                    review: 7,
                    check: "Tests".into()
                },
                &alles()
            ),
            Ok(Some(Ausgehend::GitHubCheckNeu {
                repo: "a/x".into(),
                nummer: 7,
                check: "Tests".into()
            }))
        );
    }

    #[test]
    fn review_enthaelt_urteil_text_vorschlaege_und_zeilenkommentare() {
        let d = review_daten(&review(), "  Insgesamt gut  ", "abc123");
        assert_eq!(d.urteil, "REQUEST_CHANGES");
        // Freigegebener Vorschlag mit Ort als Positionskommentar, abgelehnter fehlt, ortloser im Text.
        assert_eq!(d.kommentare.len(), 3);
        assert_eq!(
            d.kommentare[0],
            ZeilenKommentar {
                pfad: "a.rs".into(),
                zeile: 2,
                seite: "RIGHT",
                text: "Name prüfen".into()
            }
        );
        assert_eq!(
            d.kommentare[1].text,
            "Besser so\n\n```suggestion\n    neuer();\n```"
        );
        assert_eq!(d.kommentare[1].seite, "RIGHT");
        assert_eq!(d.kommentare[2].zeile, 1);
        assert!(d.text.starts_with("Insgesamt gut"));
        assert!(d.text.contains("unklar: Allgemein"));
        assert!(!d.text.contains("abgelehnt"));
        let body = d.body();
        assert_eq!(body["event"], "REQUEST_CHANGES");
        assert_eq!(body["commit_id"], "abc123");
        assert_eq!(body["comments"].as_array().unwrap().len(), 3);
        assert_eq!(body["comments"][1]["line"], 2);
    }

    #[test]
    fn kommentar_an_entfernter_zeile_geht_nach_links() {
        let mut r = review();
        r.line_comments.clear();
        r.add_line_comment("a.rs", 1, "weg damit", None).unwrap();
        let d = review_daten(&r, "", "");
        let k = d.kommentare.last().unwrap();
        assert_eq!((k.seite, k.zeile), ("LEFT", 2));
        assert!(d.body().get("commit_id").is_none());
    }

    #[test]
    fn leeres_review_bekommt_einen_text_wo_github_ihn_verlangt() {
        let mut r = review();
        r.suggestions.clear();
        r.line_comments.clear();
        r.verdict = Verdict::RequestChanges;
        assert_eq!(review_daten(&r, "", "").text, "Änderungen angefordert.");
        r.verdict = Verdict::Approve;
        assert_eq!(review_daten(&r, "", "").text, "");
        assert!(review_daten(&r, "", "").body().get("body").is_none());
        r.verdict = Verdict::Comment;
        assert_eq!(review_daten(&r, "", "").text, "Review aus dem Cockpit.");
    }

    #[test]
    fn transitionen_werden_nach_ziel_status_gewaehlt() {
        let liste: Vec<Transition> = serde_json::from_value(json!([
            {"id": "11", "name": "Start", "to": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}}},
            {"id": "21", "name": "Prüfen", "to": {"name": "In Review", "statusCategory": {"key": "indeterminate"}}},
            {"id": "31", "name": "Fertig", "to": {"name": "Done", "statusCategory": {"key": "done"}}}
        ]))
        .unwrap();
        assert_eq!(
            waehle_transition(&liste, TicketStatus::InProgress)
                .unwrap()
                .id,
            "11"
        );
        assert_eq!(
            waehle_transition(&liste, TicketStatus::Review).unwrap().id,
            "21"
        );
        assert_eq!(
            waehle_transition(&liste, TicketStatus::Done).unwrap().id,
            "31"
        );
        assert!(waehle_transition(&liste, TicketStatus::Open).is_none());
        let a = transition_anfrage("ABC-1", "31");
        assert_eq!(a.pfad, "/rest/api/3/issue/ABC-1/transitions");
        assert_eq!(a.body, Some(json!({"transition": {"id": "31"}})));
    }

    #[test]
    fn branch_und_check_aufrufe() {
        let a = branch_anfrage("a/x", "feature/x", "deadbeef");
        assert_eq!(a.pfad, "/repos/a/x/git/refs");
        assert_eq!(
            a.body,
            Some(json!({"ref": "refs/heads/feature/x", "sha": "deadbeef"}))
        );
        let c = check_neu_anfrage("a/x", 99);
        assert_eq!(
            (c.methode, c.pfad.as_str(), c.body),
            ("POST", "/repos/a/x/check-runs/99/rerequest", None)
        );
    }
}
