//! Writing to GitHub Projects: which call each action becomes, and what goes over the wire
//! (wiremock only; the answers have the shape of the documented schema).

use cockpit_core::{Action, Actor, FieldInput, Person, TicketStatus};
use cockpit_integrations::github::projects::{Assignee, IssueWrite};
use cockpit_integrations::outbound::{Outbound, PlanError, Targets, plane};
use serde_json::{Value, json};

use super::projects::{client, count_calls, gql, proj, project_server, svc};
use super::*;

fn targets() -> Targets {
    Targets {
        github: true,
        projects: true,
        ..Default::default()
    }
}

async fn synced() -> (MockServer, Workspace) {
    let s = project_server().await;
    let (ws, _) = sync(&svc(&s), &Workspace::default(), NOW_MS).await;
    (s, ws)
}

fn plan(ws: &Workspace, a: Action, t: &Targets) -> Result<Option<Outbound>, PlanError> {
    let mut after = ws.clone();
    // The rules apply first, as in the server.
    after.apply(a.clone(), Actor::User, NOW_MS).unwrap();
    plane(ws, &after, &a, t)
}

fn body_of(r: &wiremock::Request) -> Value {
    serde_json::from_slice(&r.body).unwrap_or(Value::Null)
}

async fn calls(s: &MockServer) -> Vec<(String, String, Value)> {
    s.received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| (r.method.to_string(), r.url.path().to_string(), body_of(r)))
        .collect()
}

const ISSUE: &str = "ghi-acme-app-12";

// ---------- Planning ----------

#[tokio::test]
async fn the_status_becomes_the_option_of_the_matching_column() {
    let (_s, ws) = synced().await;
    let call = plan(
        &ws,
        Action::SetStatus {
            item: ISSUE.into(),
            status: TicketStatus::Review,
        },
        &targets(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        call,
        Outbound::ProjectField {
            project_id: "PVT_acme3".into(),
            item_id: "PVTI_12".into(),
            field_id: "PVTSSF_status".into(),
            value: Some(FieldInput::Select("s_rev".into())),
        }
    );
    assert_eq!(call.service(), "GitHub");
}

#[tokio::test]
async fn a_status_without_a_column_is_a_plain_message_and_nothing_goes_out() {
    let (_s, mut ws) = synced().await;
    // A project that has only Todo and Done.
    let meta = ws
        .sync
        .projects
        .iter_mut()
        .find(|m| m.id == "PVT_acme3")
        .unwrap();
    let def = meta
        .fields
        .iter_mut()
        .find(|f| f.id == "PVTSSF_status")
        .unwrap();
    def.options.retain(|o| o.name == "Todo" || o.name == "Done");
    let err = plan(
        &ws,
        Action::SetStatus {
            item: ISSUE.into(),
            status: TicketStatus::Review,
        },
        &targets(),
    )
    .unwrap_err();
    assert!(
        err.1.render(cockpit_i18n::Lang::En).contains("no column"),
        "{}",
        err.0
    );
}

#[tokio::test]
async fn a_ticket_of_a_project_is_not_changed_when_projects_is_not_connected() {
    let (_s, ws) = synced().await;
    let off = Targets {
        github: true,
        projects: false,
        ..Default::default()
    };
    let err = plan(
        &ws,
        Action::Reply {
            item: ISSUE.into(),
            text: "Hi".into(),
        },
        &off,
    )
    .unwrap_err();
    assert!(
        err.1
            .render(cockpit_i18n::Lang::En)
            .contains("not connected")
    );
}

#[tokio::test]
async fn assigning_maps_me_nobody_and_a_login() {
    let (_s, ws) = synced().await;
    let who = |p: Person| match plan(
        &ws,
        Action::AssignTicket {
            item: ISSUE.into(),
            person: p,
        },
        &targets(),
    )
    .unwrap()
    .unwrap()
    {
        Outbound::Issue {
            repo,
            number,
            write: IssueWrite::Assign(a),
        } => {
            assert_eq!((repo.as_str(), number), ("acme/app", 12));
            a
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(
        who(Person::new("tobias-lang", "TL")),
        Assignee::Login("tobias-lang".into())
    );
    assert_eq!(
        who(Person::new(cockpit_core::UNASSIGNED_NAME, "–")),
        Assignee::Nobody
    );
    // The issue is already mine; taking it again changes nothing, so the rules stop it first.
    let mut other = ws.clone();
    other
        .items
        .iter_mut()
        .find(|i| i.id == "ghi-acme-web-15")
        .unwrap()
        .owner = Person::new("x", "X");
    let me = plan(
        &other,
        Action::AssignTicket {
            item: "ghi-acme-web-15".into(),
            person: Person::me(),
        },
        &targets(),
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        me,
        Outbound::Issue {
            write: IssueWrite::Assign(Assignee::Me),
            ..
        }
    ));
}

#[tokio::test]
async fn a_draft_cannot_be_commented_or_assigned_but_its_text_can_be_edited() {
    let (_s, ws) = synced().await;
    let draft = ws
        .items
        .iter()
        .find(|i| i.title == "Think about dark mode")
        .unwrap()
        .id
        .clone();
    let reply = plan(
        &ws,
        Action::Reply {
            item: draft.clone(),
            text: "x".into(),
        },
        &targets(),
    )
    .unwrap_err();
    assert!(reply.1.render(cockpit_i18n::Lang::En).contains("draft"));
    let assign = plan(
        &ws,
        Action::AssignTicket {
            item: draft.clone(),
            person: Person::me(),
        },
        &targets(),
    )
    .unwrap_err();
    assert!(assign.1.render(cockpit_i18n::Lang::En).contains("draft"));
    let edit = plan(
        &ws,
        Action::EditTicket {
            item: draft,
            title: "Dark mode?".into(),
            description: "Later".into(),
        },
        &targets(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        edit,
        Outbound::DraftEdit {
            draft_id: "DI_1".into(),
            title: "Dark mode?".into(),
            body: "Later".into()
        }
    );
}

#[tokio::test]
async fn labels_state_and_text_of_an_issue_are_issue_writes() {
    let (_s, ws) = synced().await;
    let write = |a: Action| match plan(&ws, a, &targets()).unwrap().unwrap() {
        Outbound::Issue { write, .. } => write,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        write(Action::SetLabel {
            item: ISSUE.into(),
            label: " needs work ".into(),
            on: true
        }),
        IssueWrite::Label {
            label: "needs work".into(),
            on: true
        }
    );
    assert_eq!(
        write(Action::SetLabel {
            item: ISSUE.into(),
            label: "bug".into(),
            on: false
        }),
        IssueWrite::Label {
            label: "bug".into(),
            on: false
        }
    );
    assert_eq!(
        write(Action::SetIssueClosed {
            item: ISSUE.into(),
            closed: true
        }),
        IssueWrite::State { closed: true }
    );
    assert_eq!(
        write(Action::Reply {
            item: ISSUE.into(),
            text: "  Done soon ".into()
        }),
        IssueWrite::Comment("Done soon".into())
    );
    assert_eq!(
        write(Action::EditTicket {
            item: ISSUE.into(),
            title: "New".into(),
            description: "Body".into()
        }),
        IssueWrite::Edit {
            title: "New".into(),
            body: "Body".into()
        }
    );
}

#[tokio::test]
async fn priority_and_other_fields_go_to_the_project() {
    let (_s, ws) = synced().await;
    let pri = plan(
        &ws,
        Action::SetPriority {
            item: ISSUE.into(),
            priority: cockpit_core::Priority::Low,
        },
        &targets(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        pri,
        Outbound::ProjectField {
            project_id: "PVT_acme3".into(),
            item_id: "PVTI_12".into(),
            field_id: "PVTSSF_prio".into(),
            value: Some(FieldInput::Select("p_lo".into())),
        }
    );
    let field = plan(
        &ws,
        Action::SetProjectField {
            item: ISSUE.into(),
            field_id: "PVTF_est".into(),
            value: Some(FieldInput::Number("8".into())),
        },
        &targets(),
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        field,
        Outbound::ProjectField {
            value: Some(FieldInput::Number(_)),
            ..
        }
    ));
    // A priority the project has no choice for (Highest) is explained.
    let err = plan(
        &ws,
        Action::SetPriority {
            item: ISSUE.into(),
            priority: cockpit_core::Priority::Lowest,
        },
        &targets(),
    )
    .unwrap_err();
    assert!(err.1.render(cockpit_i18n::Lang::En).contains("Priority"));
}

#[tokio::test]
async fn the_rules_stop_a_wrong_field_value_before_anything_is_asked() {
    let (_s, mut ws) = synced().await;
    let mut bad = |v: FieldInput, field: &str| {
        ws.apply(
            Action::SetProjectField {
                item: ISSUE.into(),
                field_id: field.into(),
                value: Some(v),
            },
            Actor::User,
            NOW_MS,
        )
    };
    assert!(bad(FieldInput::Number("many".into()), "PVTF_est").is_err());
    assert!(bad(FieldInput::Date("20.11.2026".into()), "PVTF_due").is_err());
    assert!(bad(FieldInput::Select("nope".into()), "PVTSSF_team").is_err());
    assert!(
        bad(FieldInput::Text("x".into()), "PVTF_est").is_err(),
        "kind must fit the field"
    );
    assert!(
        bad(FieldInput::Select("s_done".into()), "PVTSSF_status").is_err(),
        "status has its own action"
    );
    assert!(bad(FieldInput::Text("x".into()), "PVTF_missing").is_err());
}

#[tokio::test]
async fn a_synced_flag_is_needed_demo_items_stay_local() {
    // An item that is not from a sync is local only.
    let ws = cockpit_core::demo::seed();
    let call = plan(
        &ws,
        Action::SetStatus {
            item: "gh-acme-app-31".into(),
            status: TicketStatus::Done,
        },
        &targets(),
    )
    .unwrap();
    assert_eq!(call, None);
}

// ---------- On the wire ----------

#[tokio::test]
async fn the_status_mutation_has_the_documented_shape() {
    let s = MockServer::start().await;
    gql(
        &s,
        "updateProjectV2ItemFieldValue",
        r#"{"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"PVTI_12"}}}}"#,
    )
    .await;
    let c = client(&s, "ghp_x");
    c.set_field(
        "PVT_acme3",
        "PVTI_12",
        "PVTSSF_status",
        Some(&FieldInput::Select("s_rev".into())),
    )
    .await
    .unwrap();
    let all = calls(&s).await;
    assert_eq!(all.len(), 1);
    let (m, p, b) = &all[0];
    assert_eq!((m.as_str(), p.as_str()), ("POST", "/graphql"));
    assert!(
        b["query"]
            .as_str()
            .unwrap()
            .contains("updateProjectV2ItemFieldValue")
    );
    assert_eq!(
        b["variables"],
        json!({"project":"PVT_acme3","item":"PVTI_12","field":"PVTSSF_status",
               "value":{"singleSelectOptionId":"s_rev"}})
    );
}

#[tokio::test]
async fn every_kind_of_field_value_has_its_json() {
    use cockpit_integrations::github::projects::field_value_json as j;
    assert_eq!(
        j(&FieldInput::Iteration("it_21".into())),
        json!({"iterationId":"it_21"})
    );
    assert_eq!(j(&FieldInput::Text(" hi ".into())), json!({"text":"hi"}));
    assert_eq!(j(&FieldInput::Number("2.5".into())), json!({"number":2.5}));
    assert_eq!(
        j(&FieldInput::Date("2026-11-20".into())),
        json!({"date":"2026-11-20"})
    );
}

#[tokio::test]
async fn clearing_a_field_uses_the_clear_mutation() {
    let s = MockServer::start().await;
    gql(
        &s,
        "clearProjectV2ItemFieldValue",
        r#"{"data":{"clearProjectV2ItemFieldValue":{"projectV2Item":{"id":"PVTI_12"}}}}"#,
    )
    .await;
    client(&s, "ghp_x")
        .set_field("PVT_1", "PVTI_12", "PVTF_due", None)
        .await
        .unwrap();
    assert!(
        calls(&s).await[0].2["query"]
            .as_str()
            .unwrap()
            .contains("clearProjectV2ItemFieldValue")
    );
}

async fn rest(s: &MockServer, verb: &str, p: &str, status: u16, body: &str) {
    Mock::given(method(verb))
        .and(path(p))
        .respond_with(
            ResponseTemplate::new(status).set_body_raw(body.to_string(), "application/json"),
        )
        .mount(s)
        .await;
}

#[tokio::test]
async fn issue_writes_go_to_the_documented_rest_calls() {
    let s = MockServer::start().await;
    rest(&s, "POST", "/repos/acme/app/issues/12/comments", 201, "{}").await;
    rest(&s, "PATCH", "/repos/acme/app/issues/12", 200, "{}").await;
    rest(&s, "POST", "/repos/acme/app/issues/12/labels", 200, "[]").await;
    rest(
        &s,
        "DELETE",
        "/repos/acme/app/issues/12/labels/needs%20work%2Fmore",
        200,
        "[]",
    )
    .await;
    rest(
        &s,
        "DELETE",
        "/repos/acme/app/issues/12/labels/gone",
        404,
        r#"{"message":"Label does not exist"}"#,
    )
    .await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    let c = client(&s, "ghp_x");
    let w = |w: IssueWrite| {
        let c = &c;
        async move { c.issue_write("acme/app", 12, &w).await }
    };
    w(IssueWrite::Comment("Looks good".into())).await.unwrap();
    w(IssueWrite::State { closed: true }).await.unwrap();
    w(IssueWrite::State { closed: false }).await.unwrap();
    w(IssueWrite::Edit {
        title: "T".into(),
        body: "B".into(),
    })
    .await
    .unwrap();
    w(IssueWrite::Assign(Assignee::Me)).await.unwrap();
    w(IssueWrite::Assign(Assignee::Nobody)).await.unwrap();
    w(IssueWrite::Assign(Assignee::Login("tobias-lang".into())))
        .await
        .unwrap();
    w(IssueWrite::Label {
        label: "bug".into(),
        on: true,
    })
    .await
    .unwrap();
    w(IssueWrite::Label {
        label: "needs work/more".into(),
        on: false,
    })
    .await
    .unwrap();
    // A label that is gone already is what was wanted.
    w(IssueWrite::Label {
        label: "gone".into(),
        on: false,
    })
    .await
    .unwrap();

    let all: Vec<_> = calls(&s)
        .await
        .into_iter()
        .filter(|(_, p, _)| p != "/user")
        .collect();
    let bodies: Vec<&Value> = all.iter().map(|(_, _, b)| b).collect();
    assert_eq!(*bodies[0], json!({"body":"Looks good"}));
    assert_eq!(
        *bodies[1],
        json!({"state":"closed","state_reason":"completed"})
    );
    assert_eq!(*bodies[2], json!({"state":"open"}));
    assert_eq!(*bodies[3], json!({"title":"T","body":"B"}));
    assert_eq!(
        *bodies[4],
        json!({"assignees":["mkeller"]}),
        "me is resolved by GET /user"
    );
    assert_eq!(*bodies[5], json!({"assignees":[]}));
    assert_eq!(*bodies[6], json!({"assignees":["tobias-lang"]}));
    assert_eq!(*bodies[7], json!({"labels":["bug"]}));
    // Nothing is ever deleted except a label on request.
    assert!(
        all.iter()
            .filter(|(m, ..)| m == "DELETE")
            .all(|(_, p, _)| p.contains("/labels/"))
    );
    // A path that is no repo never leaves the program.
    assert!(
        c.issue_write("../x", 1, &IssueWrite::Comment("x".into()))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_refused_issue_write_names_the_issues_permission() {
    let s = MockServer::start().await;
    rest(
        &s,
        "POST",
        "/repos/acme/app/issues/12/comments",
        403,
        r#"{"message":"Resource not accessible by personal access token"}"#,
    )
    .await;
    let err = client(&s, "github_pat_x")
        .issue_write("acme/app", 12, &IssueWrite::Comment("x".into()))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        Error::MissingRight {
            right: cockpit_core::Right::IssuesWrite,
            kind: cockpit_core::TokenKind::FineGrained
        }
    );
    let text = err.msg().render(cockpit_i18n::Lang::En);
    assert!(
        text.contains("\"Issues\"") && text.contains("Read and write"),
        "{text}"
    );
}

#[tokio::test]
async fn a_refused_mutation_names_the_projects_write_right_for_each_kind() {
    let s = MockServer::start().await;
    // Classic token: the message names the scope the field needs.
    Mock::given(method("POST")).and(path("/graphql"))
        .respond_with(ok(r#"{"data":null,"errors":[{"type":"INSUFFICIENT_SCOPES","path":["updateProjectV2ItemFieldValue"],"message":"Your token has not been granted the required scopes to execute this query. The 'updateProjectV2ItemFieldValue' field requires one of the following scopes: ['project'], but your token has only been granted the: ['read:project', 'repo'] scopes."}]}"#))
        .mount(&s).await;
    let err = client(&s, "ghp_x")
        .set_field("P", "I", "F", None)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        Error::MissingRight {
            right: cockpit_core::Right::ProjectsWrite,
            kind: cockpit_core::TokenKind::Classic
        }
    );
    assert!(
        err.msg()
            .render(cockpit_i18n::Lang::En)
            .contains("`project`")
    );
    // Fine-grained token and GitHub App: only the refusal.
    let s2 = MockServer::start().await;
    gql(&s2, "mutation", fixture!("gh_err_forbidden.json")).await;
    let fine = client(&s2, "github_pat_x")
        .set_field("P", "I", "F", None)
        .await
        .unwrap_err();
    assert_eq!(
        fine,
        Error::MissingRight {
            right: cockpit_core::Right::ProjectsWrite,
            kind: cockpit_core::TokenKind::FineGrained
        }
    );
    let app = client(&s2, "ghu_x")
        .with_app(true)
        .set_field("P", "I", "F", None)
        .await
        .unwrap_err();
    assert_eq!(
        app,
        Error::MissingRight {
            right: cockpit_core::Right::ProjectsWrite,
            kind: cockpit_core::TokenKind::App
        }
    );
    assert!(
        app.msg()
            .render(cockpit_i18n::Lang::De)
            .contains("GitHub App")
    );
}

#[tokio::test]
async fn a_draft_is_edited_and_converted_through_graphql() {
    let s = MockServer::start().await;
    gql(
        &s,
        "updateProjectV2DraftIssue",
        r#"{"data":{"updateProjectV2DraftIssue":{"draftIssue":{"id":"DI_1"}}}}"#,
    )
    .await;
    gql(
        &s,
        "repository(owner",
        r#"{"data":{"repository":{"id":"R_app","nameWithOwner":"acme/app"}}}"#,
    )
    .await;
    gql(&s, "convertProjectV2DraftIssueItemToIssue", r#"{"data":{"convertProjectV2DraftIssueItemToIssue":{"item":{"id":"PVTI_draft1","content":{"id":"I_77","number":77,"url":"https://github.com/acme/app/issues/77","repository":{"nameWithOwner":"acme/app"}}}}}}"#).await;
    let c = client(&s, "ghp_x");
    c.update_draft("DI_1", "New title", "Text").await.unwrap();
    let made = c.convert_draft("PVTI_draft1", "acme/app").await.unwrap();
    assert_eq!(
        (made.repo.as_str(), made.number, made.content_id.as_str()),
        ("acme/app", 77, "I_77")
    );
    assert_eq!(made.item_id, "PVTI_draft1");
    let all = calls(&s).await;
    let convert = all
        .iter()
        .find(|(_, _, b)| {
            b["query"]
                .as_str()
                .unwrap_or("")
                .contains("convertProjectV2")
        })
        .unwrap();
    assert_eq!(
        convert.2["variables"],
        json!({"item":"PVTI_draft1","repo":"R_app"})
    );
}

#[tokio::test]
async fn a_new_issue_is_created_added_to_the_project_and_given_its_status() {
    let s = MockServer::start().await;
    rest(&s, "POST", "/repos/acme/app/issues", 201,
        r#"{"number":99,"node_id":"I_99","html_url":"https://github.com/acme/app/issues/99","title":"New"}"#).await;
    gql(
        &s,
        "addProjectV2ItemById",
        r#"{"data":{"addProjectV2ItemById":{"item":{"id":"PVTI_99"}}}}"#,
    )
    .await;
    gql(
        &s,
        "updateProjectV2ItemFieldValue",
        r#"{"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"PVTI_99"}}}}"#,
    )
    .await;
    let made = client(&s, "ghp_x")
        .create_issue_in_project(
            "PVT_acme3",
            "acme/app",
            "New",
            "Body",
            Some(("PVTSSF_status", "s_todo")),
        )
        .await
        .unwrap();
    assert_eq!(
        (made.number, made.content_id.as_str(), made.item_id.as_str()),
        (99, "I_99", "PVTI_99")
    );
    let all = calls(&s).await;
    let order: Vec<&str> = all
        .iter()
        .map(|(_, p, b)| {
            if p == "/graphql" {
                b["query"].as_str().unwrap().split('(').next().unwrap()
            } else {
                p.as_str()
            }
        })
        .collect();
    assert_eq!(order[0], "/repos/acme/app/issues");
    assert!(
        order[1].contains("addProjectV2ItemById")
            || all[1].2["query"]
                .as_str()
                .unwrap()
                .contains("addProjectV2ItemById")
    );
    assert!(
        all[2].2["query"]
            .as_str()
            .unwrap()
            .contains("updateProjectV2ItemFieldValue")
    );
    assert_eq!(all[0].2, json!({"title":"New","body":"Body"}));
    assert_eq!(
        all[1].2["variables"],
        json!({"project":"PVT_acme3","content":"I_99"})
    );
}

#[tokio::test]
async fn a_project_is_created_linked_and_gets_its_four_columns() {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(
        &s,
        "repositoryOwner(login: $login) { id login }",
        r#"{"data":{"repositoryOwner":{"id":"O_acme","login":"acme"}}}"#,
    )
    .await;
    gql(&s, "createProjectV2(", r#"{"data":{"createProjectV2":{"projectV2":{"id":"PVT_new","number":7,"title":"Board","url":"https://github.com/orgs/acme/projects/7","closed":false,"owner":{"__typename":"Organization","login":"acme"}}}}}"#).await;
    gql(
        &s,
        "repository(owner",
        r#"{"data":{"repository":{"id":"R_app","nameWithOwner":"acme/app"}}}"#,
    )
    .await;
    gql(
        &s,
        "linkProjectV2ToRepository",
        r#"{"data":{"linkProjectV2ToRepository":{"repository":{"id":"R_app"}}}}"#,
    )
    .await;
    // GitHub's new board comes with a Status field of three columns.
    gql(&s, "repositoryOwner(login: $login) { ... on ProjectV2Owner", r#"{"data":{"repositoryOwner":{"projectV2":{"id":"PVT_new","number":7,"title":"Board","url":"u","closed":false,"fields":{"nodes":[
        {"__typename":"ProjectV2SingleSelectField","id":"F_status","name":"Status","dataType":"SINGLE_SELECT","options":[{"id":"a","name":"Todo"},{"id":"b","name":"In Progress"},{"id":"c","name":"Done"}]}]}}}}}"#).await;
    gql(
        &s,
        "updateProjectV2Field",
        r#"{"data":{"updateProjectV2Field":{"projectV2Field":{"id":"F_status"}}}}"#,
    )
    .await;
    let p = client(&s, "ghp_x")
        .create_project("acme", "Board", Some("acme/app"))
        .await
        .unwrap();
    assert_eq!(
        (p.owner.as_str(), p.number, p.id.as_str()),
        ("acme", 7, "PVT_new")
    );
    assert_eq!(p.repos, ["acme/app"]);
    assert_eq!(p.origin, cockpit_core::ProjectOrigin::Repo);

    let all = calls(&s).await;
    let q = |needle: &str| {
        all.iter()
            .find(|(_, _, b)| b["query"].as_str().unwrap_or("").contains(needle))
    };
    assert_eq!(
        q("createProjectV2(").unwrap().2["variables"],
        json!({"owner":"O_acme","title":"Board"})
    );
    assert_eq!(
        q("linkProjectV2ToRepository").unwrap().2["variables"],
        json!({"project":"PVT_new","repo":"R_app"})
    );
    let fix = q("updateProjectV2Field").unwrap();
    let names: Vec<&str> = fix.2["variables"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Todo", "In Progress", "In Review", "Done"]);
    assert_eq!(fix.2["variables"]["field"], "F_status");
    // Cockpit never deletes a project or anything else here.
    assert!(
        all.iter()
            .all(|(m, _, b)| m != "DELETE" && !b.to_string().to_lowercase().contains("delete"))
    );
}

#[tokio::test]
async fn a_project_without_a_status_field_gets_one() {
    let s = MockServer::start().await;
    get(&s, "/user", ok(fixture!("github_user.json"))).await;
    gql(
        &s,
        "repositoryOwner(login: $login) { id login }",
        r#"{"data":{"repositoryOwner":{"id":"U_me","login":"mkeller"}}}"#,
    )
    .await;
    gql(&s, "createProjectV2(", r#"{"data":{"createProjectV2":{"projectV2":{"id":"PVT_new","number":2,"title":"Mine","url":"u","closed":false,"owner":{"__typename":"User","login":"mkeller"}}}}}"#).await;
    gql(&s, "repositoryOwner(login: $login) { ... on ProjectV2Owner", r#"{"data":{"repositoryOwner":{"projectV2":{"id":"PVT_new","number":2,"title":"Mine","url":"u","closed":false,"fields":{"nodes":[]}}}}}"#).await;
    gql(&s, "createProjectV2Field", r#"{"data":{"createProjectV2Field":{"projectV2Field":{"id":"F","name":"Status","options":[]}}}}"#).await;
    let p = client(&s, "ghp_x")
        .create_project("mkeller", "Mine", None)
        .await
        .unwrap();
    assert_eq!(p.origin, cockpit_core::ProjectOrigin::User);
    assert!(p.repos.is_empty());
    assert_eq!(
        count_calls(&s, "linkProjectV2ToRepository").await,
        0,
        "no repo, no link"
    );
    assert_eq!(count_calls(&s, "createProjectV2Field").await, 1);
    let all = calls(&s).await;
    let create = all
        .iter()
        .find(|(_, _, b)| {
            b["query"]
                .as_str()
                .unwrap_or("")
                .contains("createProjectV2Field")
        })
        .unwrap();
    assert_eq!(
        create.2["variables"]["options"].as_array().unwrap().len(),
        4
    );
}

#[tokio::test]
async fn creating_a_project_without_the_right_names_the_right() {
    let s = MockServer::start().await;
    gql(
        &s,
        "repositoryOwner(login: $login) { id login }",
        r#"{"data":{"repositoryOwner":{"id":"O_acme","login":"acme"}}}"#,
    )
    .await;
    gql(&s, "createProjectV2(", fixture!("gh_err_forbidden.json")).await;
    let err = client(&s, "github_pat_x")
        .create_project("acme", "Board", None)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        Error::MissingRight {
            right: cockpit_core::Right::ProjectsWrite,
            kind: cockpit_core::TokenKind::FineGrained
        }
    );
    let _ = proj("a", 1);
}

use cockpit_integrations::Error;
