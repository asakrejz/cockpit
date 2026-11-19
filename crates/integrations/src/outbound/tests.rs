use super::*;
use cockpit_core::{
    Actor, Checks, Comment, DiffLine, FileDiff, Item, Kind, Loaded, Outcome, Person, PrLink, PrRef,
    ReasonKind, Remote, Scope, Source, SourceData, Suggestion,
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
        description_rich: None,
        reason: ReasonKind::Assigned,
        reason_text: String::new(),
        time: String::new(),
        scope: Scope::Mine,
        owner: Person::me(),
        status: TicketStatus::Open,
        branch: None,
        pr: (source == Source::GitHub).then_some(PrRef {
            number: 7,
            repo: "a/x".into(),
            draft: false,
            checks: Checks::Passed,
            changes_requested: false,
            merged: false,
            review_requested: false,
            author: String::new(),
            mine: false,
            requested_me: false,
            assigned_me: false,
        }),
        agent_note: None,
        agent_note_msg: None,
        comments: Vec::<Comment>::new(),
        read: false,
        priority: Default::default(),
        due: None,
        repo: None,
        labels: Vec::new(),
        sprint: None,
        issue_type: None,
        updated_ms: None,
        url: None,
        details: None,
    }
}

fn review() -> Review {
    let mut r = cockpit_core::demo::workspace().reviews.remove(0);
    r.number = 7;
    r.repo = "a/x".into();
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
                    warning_msgs: vec![],
                }),
            },
        ],
    };
    cockpit_core::sync::merge(&Workspace::default(), &remote, T).0
}

fn all() -> Targets {
    Targets {
        github: true,
        jira: true,
        repos: vec!["a/x".into()],
        routing: Default::default(),
    }
}

fn plan(ws: &Workspace, a: Action, z: &Targets) -> Result<Option<Outbound>, PlanError> {
    let mut after = ws.clone();
    let _ = after.apply(a.clone(), Actor::User, T);
    plane(ws, &after, &a, z)
}

#[test]
fn reply_goes_to_the_right_service_per_source() {
    let w = ws();
    let a = plan(
        &w,
        Action::Reply {
            item: "abc-1".into(),
            text: " Hallo ".into(),
        },
        &all(),
    );
    assert_eq!(
        a,
        Ok(Some(Outbound::JiraComment {
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
        &all(),
    );
    assert_eq!(
        a,
        Ok(Some(Outbound::GitHubComment {
            repo: "a/x".into(),
            number: 7,
            text: "Hi".into()
        }))
    );
    let request = a.unwrap().unwrap().request().unwrap();
    assert_eq!(
        (request.method, request.path.as_str(), request.body),
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
        &all(),
    )
    .unwrap()
    .unwrap()
    .request()
    .unwrap();
    assert_eq!(jira.path, "/rest/api/3/issue/ABC-1/comment");
    assert_eq!(
        jira.body.unwrap()["body"]["content"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn local_stays_local() {
    let w = ws();
    let demo = cockpit_core::demo::workspace();
    // Sample data: never to the outside.
    let a = plan(
        &demo,
        Action::Reply {
            item: "bau142".into(),
            text: "x".into(),
        },
        &all(),
    );
    assert_eq!(a, Ok(None));
    // Source off.
    let from = Targets::default();
    assert_eq!(
        plan(
            &w,
            Action::Reply {
                item: "abc-1".into(),
                text: "x".into()
            },
            &from
        ),
        Ok(None)
    );
    // Other actions.
    assert_eq!(
        plan(
            &w,
            Action::MarkDone {
                item: "abc-1".into()
            },
            &all()
        ),
        Ok(None)
    );
    assert_eq!(plan(&w, Action::AdvanceAgents, &all()), Ok(None));
    assert_eq!(
        plan(
            &w,
            Action::SetVerdict {
                review: 7.into(),
                verdict: Verdict::Approve
            },
            &all()
        ),
        Ok(None)
    );
}

#[test]
fn status_branch_merge_and_check() {
    let w = ws();
    assert_eq!(
        plan(
            &w,
            Action::SetStatus {
                item: "abc-1".into(),
                status: TicketStatus::Done
            },
            &all()
        ),
        Ok(Some(Outbound::JiraStatus {
            key: "ABC-1".into(),
            status: TicketStatus::Done
        }))
    );
    let Ok(Some(Outbound::GitHubBranch { repo, name })) = plan(
        &w,
        Action::CreateBranch {
            item: "abc-1".into(),
        },
        &all(),
    ) else {
        panic!("expected branch");
    };
    assert_eq!(repo, "a/x");
    assert_eq!(name, "feature/ABC-1-avatar-groesse");
    let without_repo = Targets {
        repos: vec![],
        ..all()
    };
    assert!(
        plan(
            &w,
            Action::CreateBranch {
                item: "abc-1".into()
            },
            &without_repo
        )
        .is_err()
    );

    let m = plan(
        &w,
        Action::MergePr {
            review: 7.into(),
            method: MergeMethod::Squash,
        },
        &all(),
    );
    let req = m.unwrap().unwrap().request().unwrap();
    assert_eq!(
        (req.method, req.path.as_str()),
        ("PUT", "/repos/a/x/pulls/7/merge")
    );
    assert_eq!(
        req.body,
        Some(json!({"merge_method": "squash", "sha": "abc123"}))
    );

    assert_eq!(
        plan(
            &w,
            Action::RestartCheck {
                review: 7.into(),
                check: "Tests".into()
            },
            &all()
        ),
        Ok(Some(Outbound::GitHubRecheck {
            repo: "a/x".into(),
            number: 7,
            check: "Tests".into()
        }))
    );
}

#[test]
fn review_contains_verdict_text_suggestions_and_line_comments() {
    let d = review_data(&review(), "  Insgesamt gut  ", "abc123");
    assert_eq!(d.verdict, "REQUEST_CHANGES");
    // Approved suggestion with location as a positioned comment, rejected one is missing, locationless one in the text.
    assert_eq!(d.comments.len(), 3);
    assert_eq!(
        d.comments[0],
        LineComment {
            path: "a.rs".into(),
            line: 2,
            page: "RIGHT",
            start_line: None,
            text: "Name prüfen".into()
        }
    );
    assert_eq!(
        d.comments[1].text,
        "Besser so\n\n```suggestion\n    neuer();\n```"
    );
    assert_eq!(d.comments[1].page, "RIGHT");
    assert_eq!(d.comments[2].line, 1);
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
fn comment_on_removed_line_goes_left() {
    let mut r = review();
    r.line_comments.clear();
    r.add_line_comment("a.rs", 1, "weg damit", None).unwrap();
    let d = review_data(&r, "", "");
    let k = d.comments.last().unwrap();
    assert_eq!((k.page, k.line), ("LEFT", 2));
    assert!(d.body().get("commit_id").is_none());
}

#[test]
fn empty_review_gets_a_text_where_github_requires_one() {
    let mut r = review();
    r.suggestions.clear();
    r.line_comments.clear();
    r.verdict = Verdict::RequestChanges;
    assert_eq!(review_data(&r, "", "").text, "Changes requested.");
    r.verdict = Verdict::Approve;
    assert_eq!(review_data(&r, "", "").text, "");
    assert!(review_data(&r, "", "").body().get("body").is_none());
    r.verdict = Verdict::Comment;
    assert_eq!(review_data(&r, "", "").text, "Review from Cockpit.");
}

#[test]
fn transitions_are_chosen_by_target_status() {
    let list: Vec<Transition> = serde_json::from_value(json!([
        {"id": "11", "name": "Start", "to": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}}},
        {"id": "21", "name": "Prüfen", "to": {"name": "In Review", "statusCategory": {"key": "indeterminate"}}},
        {"id": "31", "name": "Fertig", "to": {"name": "Done", "statusCategory": {"key": "done"}}}
    ]))
    .unwrap();
    assert_eq!(
        choose_transition(&list, TicketStatus::InProgress)
            .unwrap()
            .id,
        "11"
    );
    assert_eq!(
        choose_transition(&list, TicketStatus::Review).unwrap().id,
        "21"
    );
    assert_eq!(
        choose_transition(&list, TicketStatus::Done).unwrap().id,
        "31"
    );
    assert!(choose_transition(&list, TicketStatus::Open).is_none());
    let a = transition_request("ABC-1", "31");
    assert_eq!(a.path, "/rest/api/3/issue/ABC-1/transitions");
    assert_eq!(a.body, Some(json!({"transition": {"id": "31"}})));
}

#[test]
fn branch_and_check_calls() {
    let a = branch_request("a/x", "feature/x", "deadbeef");
    assert_eq!(a.path, "/repos/a/x/git/refs");
    assert_eq!(
        a.body,
        Some(json!({"ref": "refs/heads/feature/x", "sha": "deadbeef"}))
    );
    let c = check_again_request("a/x", 99);
    assert_eq!(
        (c.method, c.path.as_str(), c.body),
        ("POST", "/repos/a/x/check-runs/99/rerequest", None)
    );
}

// ---------- Equal numbers from two repos ----------

/// `#7` in `a/x` (the usual one) and `#7` in `b/y`.
fn ws_two() -> Workspace {
    let mut other = review();
    other.repo = "b/y".into();
    other.title = "Anderes Repo".into();
    let mut other_item = item("gh-b-y-7", "#7", Source::GitHub);
    other_item.pr.as_mut().unwrap().repo = "b/y".into();
    let mut w = ws();
    let remote = Remote {
        sources: vec![SourceData {
            source: Source::GitHub,
            outcome: Outcome::Loaded(Loaded {
                items: vec![item("gh-a-x-7", "#7", Source::GitHub), other_item],
                reviews: vec![review(), other],
                prs: vec![
                    PrLink {
                        number: 7,
                        repo: "a/x".into(),
                        head_sha: "abc123".into(),
                    },
                    PrLink {
                        number: 7,
                        repo: "b/y".into(),
                        head_sha: "def456".into(),
                    },
                ],
                warnings: vec![],
                warning_msgs: vec![],
            }),
        }],
    };
    w = cockpit_core::sync::merge(&w, &remote, T).0;
    w
}

#[test]
fn a_merge_goes_to_the_repo_of_the_pull_request() {
    let w = ws_two();
    let mut targets = all();
    targets.repos = vec!["a/x".into(), "b/y".into()];
    for (repo, sha) in [("a/x", "abc123"), ("b/y", "def456")] {
        let call = plan(
            &w,
            Action::MergePr {
                review: cockpit_core::PrKey::new(repo, 7),
                method: MergeMethod::Squash,
            },
            &targets,
        );
        // The pull request must be mergeable first; the plan only needs the link.
        match call {
            Ok(Some(Outbound::GitHubMerge {
                repo: r,
                number,
                sha: s,
                ..
            })) => assert_eq!((r.as_str(), number, s.as_str()), (repo, 7, sha)),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_branch_for_a_ticket_goes_to_the_repo_its_project_is_routed_to() {
    let mut w = ws();
    // ABC-1 belongs to project ABC.
    let routing = cockpit_core::AgentRepos {
        repos: vec![
            cockpit_core::AgentRepo::new("a/x"),
            cockpit_core::AgentRepo {
                projects: vec!["ABC".into()],
                ..cockpit_core::AgentRepo::new("b/y")
            },
        ],
        default: Some("a/x".into()),
    };
    let targets = Targets {
        repos: vec!["a/x".into(), "b/y".into()],
        routing,
        ..all()
    };
    w.items.iter_mut().find(|i| i.id == "abc-1").unwrap().branch = None;
    let Ok(Some(Outbound::GitHubBranch { repo, .. })) = plan(
        &w,
        Action::CreateBranch {
            item: "abc-1".into(),
        },
        &targets,
    ) else {
        panic!("a branch is planned");
    };
    assert_eq!(repo, "b/y");
}
