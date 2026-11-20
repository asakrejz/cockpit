//! GitHub project tickets inside the workspace: linking pull requests, the sprint filter, the team
//! filter, the demo seed.

use crate::demo;
use crate::model::{Filter, PrKey, Scope, Source, SourceFilter, TeamRef};
use crate::project::ProjectItem;
use crate::search::InboxFilter;
use crate::setup::Step;
use crate::{Action, Actor, TicketStatus, TicketsFrom, Workspace};

const T: u64 = 1_700_000_000_000;

/// The demo workspace plus the sample project tickets: `acme/app#31` (issue) and a draft.
fn ws() -> Workspace {
    demo::seed()
}

/// Makes pull request #85 (repo `acme/app`) say something.
fn say(w: &mut Workspace, branch: &str, title: &str, body: &str) {
    let r = w.reviews.iter_mut().find(|r| r.number == 85).unwrap();
    r.branch = branch.into();
    r.title = title.into();
    r.description = body.into();
    r.ticket_key = None;
}

#[test]
fn the_seed_has_a_project_issue_and_a_draft_and_the_base_demo_is_unchanged() {
    let w = ws();
    assert_eq!(w.items.len(), demo::multi_repo_workspace().items.len() + 4);
    // (the issue, the draft and two tickets of colleagues in Jira)
    let issue = w.item_by_key("acme/app#31").unwrap();
    assert_eq!(issue.source, Source::GitHubProject);
    assert!(issue.is_ticket() && issue.is_mine());
    assert!(
        w.items
            .iter()
            .any(|i| i.project.as_ref().is_some_and(|p| p.draft))
    );
}

#[test]
fn a_short_reference_in_the_pr_text_links_the_issue_of_the_same_repo() {
    let mut w = ws();
    w.pr_status_off = true;
    say(&mut w, "feature/focus", "Focus ring", "Fixes #31");
    let links = w.auto_links();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].0, "gh-acme-app-31");
    w.apply(Action::AutoLinkPrs, Actor::System, T).unwrap();
    assert_eq!(
        w.review(85).unwrap().ticket_key.as_deref(),
        Some("acme/app#31")
    );
    assert_eq!(w.linked_reviews("gh-acme-app-31").len(), 1);
}

#[test]
fn a_qualified_reference_links_across_repos_but_a_short_one_does_not() {
    let mut w = ws();
    // The pull request lives in another repo: `#31` there is another issue.
    w.reviews.iter_mut().find(|r| r.number == 85).unwrap().repo = "acme/web".into();
    say(&mut w, "x", "t", "relates to #31");
    assert!(w.linked_reviews("gh-acme-app-31").is_empty());
    say(&mut w, "x", "t", "Closes acme/app#31");
    assert_eq!(w.linked_reviews("gh-acme-app-31").len(), 1);
}

#[test]
fn another_number_does_not_link() {
    let mut w = ws();
    say(&mut w, "x", "t", "Fixes #310 and see #3");
    assert!(w.linked_reviews("gh-acme-app-31").is_empty());
    assert!(w.auto_links().is_empty());
}

#[test]
fn the_timeline_of_the_issue_links_a_pr_that_never_names_it() {
    let mut w = ws();
    w.pr_status_off = true;
    say(&mut w, "agent/x", "Something unrelated", "");
    assert!(w.linked_reviews("gh-acme-app-31").is_empty());
    let key = w.review(85).unwrap().key();
    let issue = w
        .items
        .iter_mut()
        .find(|i| i.id == "gh-acme-app-31")
        .unwrap();
    issue.project.as_mut().unwrap().linked_prs = vec![key];
    assert_eq!(w.linked_reviews("gh-acme-app-31").len(), 1);
    assert_eq!(w.auto_links().len(), 1);
}

#[test]
fn linking_prefers_the_jira_key_but_a_jira_free_pr_finds_the_issue() {
    let mut w = ws();
    say(&mut w, "feature/bau-150-sheet", "t", "also #31");
    let links = w.auto_links();
    // One pull request links one ticket: the Jira ticket named in the branch wins.
    assert_eq!(links[0].0, "bau150");
}

#[test]
fn the_sprint_view_keeps_project_tickets_without_an_iteration() {
    let mut w = ws();
    let f = InboxFilter::default();
    let hits = w.inbox(Filter::All, SourceFilter::All, "", T, &f);
    assert!(
        hits.iter().any(|i| i.id == "gh-acme-app-31"),
        "no iteration: shown"
    );
    // In a past iteration the ticket drops out under „Aktueller Sprint“.
    let issue = w
        .items
        .iter_mut()
        .find(|i| i.id == "gh-acme-app-31")
        .unwrap();
    issue.sprint = Some(crate::Sprint {
        name: "Sprint 20".into(),
        state: crate::SprintState::Closed,
        start: String::new(),
        end: String::new(),
    });
    let hits = w.inbox(Filter::All, SourceFilter::All, "", T, &f);
    assert!(hits.iter().all(|i| i.id != "gh-acme-app-31"));
}

#[test]
fn the_source_filter_separates_project_tickets() {
    let w = ws();
    let hits = crate::search::search(&w.items, Filter::All, SourceFilter::GitHubProject, "");
    assert_eq!(hits.len(), 2);
    assert!(
        crate::search::search(&w.items, Filter::All, SourceFilter::GitHub, "")
            .iter()
            .all(|i| i.source == Source::GitHub)
    );
    assert_eq!(
        SourceFilter::from_id("project"),
        Some(SourceFilter::GitHubProject)
    );
}

#[test]
fn the_team_choice_applies_to_the_team_field_of_a_project_ticket() {
    let mut w = ws();
    // Someone else's ticket in team Web is hidden when only team Core is chosen.
    let id = "gh-acme-3-draft1";
    w.items.iter_mut().find(|i| i.id == id).unwrap().team = Some(TeamRef::new("t_web", "Web"));
    assert_eq!(
        w.items.iter().find(|i| i.id == id).unwrap().scope,
        Scope::Team
    );
    w.team_choice = crate::TeamChoice {
        teams: vec![TeamRef::new("t_core", "Core")],
        include_no_team: false,
    };
    assert!(!w.team_visible(w.items.iter().find(|i| i.id == id).unwrap()));
    // My own ticket stays visible whatever its team is.
    assert!(w.team_visible(w.item("gh-acme-app-31").unwrap()));
    // A pull request that names the hidden ticket's issue is hidden with it.
    assert!(w.teams().iter().any(|t| t.id == "t_web"));
}

#[test]
fn a_pr_naming_an_issue_follows_the_team_of_that_issue() {
    let mut w = ws();
    // Issue 31 belongs to Core and is not mine; a pull request that fixes it follows its team.
    let issue = w
        .items
        .iter_mut()
        .find(|i| i.id == "gh-acme-app-31")
        .unwrap();
    issue.scope = Scope::Team;
    issue.team = Some(TeamRef::new("t_core", "Core"));
    let pr = w.items.iter_mut().find(|i| i.id == "pr85").unwrap();
    pr.scope = Scope::Team;
    pr.title = "Focus ring, fixes #31".into();
    w.team_choice = crate::TeamChoice {
        teams: vec![TeamRef::new("t_web", "Web")],
        include_no_team: false,
    };
    let pr = w.item("pr85").unwrap().clone();
    assert!(
        !w.team_visible(&pr),
        "the linked issue is hidden, so is the pull request"
    );
    w.team_choice.teams = vec![TeamRef::new("t_core", "Core")];
    assert!(w.team_visible(&pr));
}

/// The steps the assistant walks through for a choice of ticket source.
fn walk(from: TicketsFrom) -> Vec<Step> {
    let mut steps = vec![Step::Container];
    while let Some(n) = steps.last().unwrap().next_for(from) {
        steps.push(n);
    }
    steps
}

#[test]
fn the_assistant_walks_only_the_steps_of_the_chosen_source() {
    use Step as S;
    use TicketsFrom::*;
    // The choice comes early, right after the container; GitHub is a step of its own either way.
    assert_eq!(
        walk(Jira),
        [
            S::Container,
            S::Source,
            S::Jira,
            S::GitHub,
            S::Model,
            S::Live,
            S::Done
        ]
    );
    assert_eq!(
        walk(GitHubProjects),
        [
            S::Container,
            S::Source,
            S::GitHub,
            S::Projects,
            S::Model,
            S::Live,
            S::Done
        ]
    );
    assert_eq!(
        walk(Both),
        [
            S::Container,
            S::Source,
            S::Jira,
            S::GitHub,
            S::Projects,
            S::Model,
            S::Live,
            S::Done
        ]
    );
}

#[test]
fn going_back_skips_the_same_steps() {
    use TicketsFrom::*;
    for from in TicketsFrom::ALL {
        let mut forward = walk(from);
        let mut back = vec![Step::Done];
        while let Some(p) = back.last().unwrap().previous_for(from) {
            back.push(p);
        }
        back.reverse();
        assert_eq!(forward, back, "{from:?}");
        forward.clear();
    }
    assert_eq!(Step::Source.previous_for(Jira), Some(Step::Container));
    assert_eq!(Step::Container.previous_for(Jira), None);
}

#[test]
fn only_the_steps_of_the_other_source_are_hidden() {
    use TicketsFrom::*;
    assert!(Step::Jira.shown(Jira) && Step::Jira.shown(Both) && !Step::Jira.shown(GitHubProjects));
    assert!(
        Step::Projects.shown(GitHubProjects)
            && Step::Projects.shown(Both)
            && !Step::Projects.shown(Jira)
    );
    for from in TicketsFrom::ALL {
        for always in [
            Step::Container,
            Step::Source,
            Step::GitHub,
            Step::Model,
            Step::Done,
        ] {
            assert!(always.shown(from), "{always:?} for {from:?}");
        }
    }
    assert_eq!(Step::from_id("source"), Some(Step::Source));
    assert_eq!(Step::from_id("projects"), Some(Step::Projects));
}

#[test]
fn tickets_from_reads_ids_and_says_what_it_uses() {
    use TicketsFrom::*;
    assert_eq!(TicketsFrom::from_id("both"), Some(Both));
    assert_eq!(TicketsFrom::from_id("nope"), None);
    assert!(Jira.uses_jira() && !Jira.uses_projects());
    assert!(!GitHubProjects.uses_jira() && GitHubProjects.uses_projects());
    assert!(Both.uses_jira() && Both.uses_projects());
}

#[test]
fn old_saves_without_project_fields_still_load() {
    // An item as an older version wrote it: none of the new fields.
    let w = demo::workspace();
    let json = crate::persist::save(&w);
    assert!(!json.contains("co_assignees") && !json.contains("\"project\":{"));
    let (back, restored) = crate::persist::restore(Some(&json));
    assert!(restored);
    assert!(
        back.items
            .iter()
            .all(|i| i.project.is_none() && i.co_assignees.is_empty())
    );
    let _ = (ProjectItem::default(), PrKey::new("a/b", 1));
}

#[test]
fn an_issue_branch_uses_the_number_not_the_key_with_slashes() {
    use crate::branch::{BranchType, matches, render};
    let branch = render(
        "claude/{ticket}-{slug}",
        "acme/app#31",
        "Fokusring sichtbar",
        BranchType::Fix,
    );
    assert_eq!(branch, "claude/gh-31-fokusring-sichtbar");
    assert!(matches("claude/{ticket}-{slug}", &branch));
    // A Jira key stays as it was.
    assert_eq!(
        render("claude/{ticket}-{slug}", "BAU-142", "x", BranchType::Fix),
        "claude/BAU-142-x"
    );
}

#[test]
fn the_pull_request_text_of_an_issue_closes_it() {
    use crate::{AgentRun, Workspace};
    let mut w = Workspace::default();
    let mut run = AgentRun::new("acme/app#31", "Fokusring");
    run.repo = "acme/app".into();
    run.summary = "Ring added".into();
    w.runs.push(run);
    let draft = w.pull_request_draft("acme/app#31", "acme/app").unwrap();
    assert!(
        draft.body.contains("\nFixes acme/app#31\n"),
        "{}",
        draft.body
    );
    // The text the draft carries links back to the issue by its key.
    assert!(
        crate::project::issue_refs(&draft.body, "acme/app")
            .iter()
            .any(|r| r.closing && r.key() == "acme/app#31")
    );
    // A Jira ticket gets no such line.
    let mut j = Workspace::default();
    j.runs.push(AgentRun::new("BAU-142", "x"));
    assert!(
        !j.pull_request_draft("BAU-142", "")
            .unwrap()
            .body
            .contains("Fixes")
    );
}

// ---------- Writing: the local effect once GitHub said yes ----------

use crate::project::FieldValue;
use crate::{ActionError, CreatedIssue, FieldInput};

/// The demo seed; it carries the field definitions of its project, as a sync would leave them.
fn with_meta() -> Workspace {
    ws()
}

fn apply(w: &mut Workspace, a: Action) -> Result<String, ActionError> {
    w.apply(a, Actor::User, T).map(|r| r.summary)
}

const ISSUE: &str = "gh-acme-app-31";

#[test]
fn labels_are_added_and_removed_once_and_only_on_issues() {
    let mut w = ws();
    let label = |on: bool, l: &str| Action::SetLabel {
        item: ISSUE.into(),
        label: l.into(),
        on,
    };
    assert_eq!(
        apply(&mut w, label(true, " needs work ")).unwrap(),
        "Label „needs work“ zu acme/app#31 hinzugefügt"
    );
    assert!(
        w.item(ISSUE)
            .unwrap()
            .labels
            .contains(&"needs work".to_string())
    );
    assert_eq!(
        apply(&mut w, label(true, "NEEDS WORK")),
        Err(ActionError::Unchanged)
    );
    assert!(apply(&mut w, label(false, "a11y")).is_ok());
    assert!(!w.item(ISSUE).unwrap().labels.contains(&"a11y".to_string()));
    assert_eq!(
        apply(&mut w, label(false, "a11y")),
        Err(ActionError::Unchanged)
    );
    for bad in ["", "  ", "a,b"] {
        assert_eq!(
            apply(&mut w, label(true, bad)),
            Err(ActionError::InvalidInput)
        );
    }
    // A draft has no labels; a Jira ticket and a pull request are no project tickets at all.
    let draft = Action::SetLabel {
        item: "gh-acme-3-draft1".into(),
        label: "x".into(),
        on: true,
    };
    assert_eq!(apply(&mut w, draft), Err(ActionError::NotAnIssue));
    for id in ["bau142", "pr87"] {
        let a = Action::SetLabel {
            item: id.into(),
            label: "x".into(),
            on: true,
        };
        assert_eq!(apply(&mut w, a), Err(ActionError::NotATicket));
    }
}

#[test]
fn closing_and_reopening_flips_the_state_once() {
    let mut w = ws();
    let close = |closed| Action::SetIssueClosed {
        item: ISSUE.into(),
        closed,
    };
    assert_eq!(apply(&mut w, close(false)), Err(ActionError::Unchanged));
    assert_eq!(
        apply(&mut w, close(true)).unwrap(),
        "acme/app#31 geschlossen"
    );
    assert!(w.item(ISSUE).unwrap().project.as_ref().unwrap().closed);
    assert_eq!(apply(&mut w, close(true)), Err(ActionError::Unchanged));
    assert_eq!(
        apply(&mut w, close(false)).unwrap(),
        "acme/app#31 wieder geöffnet"
    );
    let draft = Action::SetIssueClosed {
        item: "gh-acme-3-draft1".into(),
        closed: true,
    };
    assert_eq!(apply(&mut w, draft), Err(ActionError::NotAnIssue));
}

#[test]
fn a_project_field_is_set_checked_and_cleared() {
    let mut w = with_meta();
    let set = |field: &str, v: Option<FieldInput>| Action::SetProjectField {
        item: ISSUE.into(),
        field_id: field.into(),
        value: v,
    };
    let value = |w: &Workspace, field: &str| -> Option<FieldValue> {
        w.item(ISSUE)
            .unwrap()
            .project
            .as_ref()
            .unwrap()
            .fields
            .iter()
            .find(|f| f.field_id == field)
            .cloned()
    };
    // Number (the demo item has 3).
    assert_eq!(
        apply(
            &mut w,
            set("PVTF_est", Some(FieldInput::Number("3".into())))
        ),
        Err(ActionError::Unchanged)
    );
    assert!(
        apply(
            &mut w,
            set("PVTF_est", Some(FieldInput::Number("5".into())))
        )
        .is_ok()
    );
    assert_eq!(value(&w, "PVTF_est").unwrap().text, "5");
    // Date, text, single select (shows the option's name).
    assert!(
        apply(
            &mut w,
            set("PVTF_due", Some(FieldInput::Date("2026-12-24".into())))
        )
        .is_ok()
    );
    assert!(
        apply(
            &mut w,
            set("PVTF_notes", Some(FieldInput::Text(" soon ".into())))
        )
        .is_ok()
    );
    assert_eq!(value(&w, "PVTF_notes").unwrap().text, "soon");
    let log = apply(
        &mut w,
        set("PVTSSF_team", Some(FieldInput::Select("t_web".into()))),
    )
    .unwrap();
    assert_eq!(log, "Team von acme/app#31 auf „Web“ gesetzt");
    assert_eq!(
        value(&w, "PVTSSF_team").unwrap().option_id.as_deref(),
        Some("t_web")
    );
    // Clearing.
    assert_eq!(
        apply(&mut w, set("PVTF_notes", None)).unwrap(),
        "Notes von acme/app#31 geleert"
    );
    assert!(value(&w, "PVTF_notes").is_none());
    assert_eq!(
        apply(&mut w, set("PVTF_notes", None)),
        Err(ActionError::Unchanged)
    );
    // Mistakes.
    for (field, v) in [
        ("PVTF_est", Some(FieldInput::Number("many".into()))),
        ("PVTF_due", Some(FieldInput::Date("24.12.2026".into()))),
        ("PVTF_est", Some(FieldInput::Text("x".into()))),
        ("PVTSSF_status", Some(FieldInput::Select("s_done".into()))),
    ] {
        assert_eq!(
            apply(&mut w, set(field, v)),
            Err(ActionError::InvalidInput),
            "{field}"
        );
    }
    assert!(matches!(
        apply(
            &mut w,
            set("PVTSSF_team", Some(FieldInput::Select("nope".into())))
        ),
        Err(ActionError::UnknownField(_))
    ));
    assert!(matches!(
        apply(&mut w, set("F_gone", None)),
        Err(ActionError::UnknownField(_))
    ));
    // A draft has fields, too.
    let d = Action::SetProjectField {
        item: "gh-acme-3-draft1".into(),
        field_id: "PVTF_est".into(),
        value: Some(FieldInput::Number("1".into())),
    };
    assert!(apply(&mut w, d).is_ok());
}

#[test]
fn only_the_user_may_write_to_a_project() {
    let mut w = ws();
    let a = Action::SetLabel {
        item: ISSUE.into(),
        label: "x".into(),
        on: true,
    };
    for actor in [Actor::Agent, Actor::System] {
        assert_eq!(
            w.apply(a.clone(), actor, T),
            Err(ActionError::UserOnly(actor))
        );
    }
    assert!(w.apply(a, Actor::User, T).is_ok());
}

fn created(repo: &str, number: u32) -> CreatedIssue {
    CreatedIssue {
        repo: repo.into(),
        number,
        content_id: format!("I_{number}"),
        item_id: format!("PVTI_{number}"),
        url: format!("https://github.com/{repo}/issues/{number}"),
    }
}

#[test]
fn a_new_issue_needs_the_answer_of_github_and_then_joins_the_project() {
    let mut w = with_meta();
    let create = |created: Option<CreatedIssue>| Action::CreateIssue {
        project: "PVT_demo".into(),
        repo: "acme/app".into(),
        title: " Neues Ticket ".into(),
        body: "Text".into(),
        status: Some(TicketStatus::Review),
        created,
    };
    // Without GitHub's answer nothing is invented.
    assert_eq!(apply(&mut w, create(None)), Err(ActionError::NeedsBackend));
    let before = w.items.len();
    let log = apply(&mut w, create(Some(created("acme/app", 40)))).unwrap();
    assert_eq!(log, "Issue acme/app#40 im Projekt „App roadmap“ angelegt");
    assert_eq!(w.items.len(), before + 1);
    let t = w.item("ghi-acme-app-40").unwrap();
    assert_eq!(
        (t.title.as_str(), t.status, t.key.as_str()),
        ("Neues Ticket", TicketStatus::Review, "acme/app#40")
    );
    assert_eq!(t.project.as_ref().unwrap().item_id, "PVTI_40");
    assert_eq!(
        t.project.as_ref().unwrap().status_option.as_deref(),
        Some("s_rev")
    );
    assert!(
        w.sync.is_synced(&t.id),
        "so that later changes reach GitHub"
    );
    // The same issue twice is refused; mistakes are named.
    assert_eq!(
        apply(&mut w, create(Some(created("acme/app", 40)))),
        Err(ActionError::Unchanged)
    );
    for (title, repo, project) in [
        ("  ", "acme/app", "PVT_demo"),
        ("x", "not a repo", "PVT_demo"),
    ] {
        let a = Action::CreateIssue {
            project: project.into(),
            repo: repo.into(),
            title: title.into(),
            body: String::new(),
            status: None,
            created: Some(created("acme/app", 41)),
        };
        assert_eq!(apply(&mut w, a), Err(ActionError::InvalidInput));
    }
    let unknown = Action::CreateIssue {
        project: "PVT_other".into(),
        repo: "acme/app".into(),
        title: "x".into(),
        body: String::new(),
        status: None,
        created: Some(created("acme/app", 42)),
    };
    assert!(matches!(
        apply(&mut w, unknown),
        Err(ActionError::UnknownField(_))
    ));
}

#[test]
fn a_converted_draft_becomes_the_issue_with_the_same_place_in_the_project() {
    let mut w = with_meta();
    let convert = |created| Action::ConvertDraft {
        item: "gh-acme-3-draft1".into(),
        repo: "acme/app".into(),
        created,
    };
    assert_eq!(apply(&mut w, convert(None)), Err(ActionError::NeedsBackend));
    w.snoozed.push(crate::Snooze {
        item: "gh-acme-3-draft1".into(),
        until_ms: None,
        comments: 0,
        status: TicketStatus::Open,
        checks: None,
    });
    w.sync.items.push("gh-acme-3-draft1".into());
    let log = apply(&mut w, convert(Some(created("acme/app", 50)))).unwrap();
    assert_eq!(log, "Entwurf in das Issue acme/app#50 umgewandelt");
    assert!(w.item("gh-acme-3-draft1").is_none());
    let t = w.item("ghi-acme-app-50").unwrap();
    assert_eq!(t.key, "acme/app#50");
    assert_eq!(t.repo.as_deref(), Some("acme/app"));
    let p = t.project.as_ref().unwrap();
    assert!(!p.draft && p.number == Some(50) && p.content_id == "I_50");
    // The snooze and the list of synced entries moved with it.
    assert!(w.snoozed.iter().all(|s| s.item == "ghi-acme-app-50"));
    assert!(w.sync.is_synced("ghi-acme-app-50") && !w.sync.is_synced("gh-acme-3-draft1"));
    // An issue cannot be converted again; a Jira ticket is no draft.
    let again = Action::ConvertDraft {
        item: "ghi-acme-app-50".into(),
        repo: "acme/app".into(),
        created: Some(created("acme/app", 51)),
    };
    assert_eq!(apply(&mut w, again), Err(ActionError::NotADraft));
    let jira = Action::ConvertDraft {
        item: "bau142".into(),
        repo: "acme/app".into(),
        created: Some(created("acme/app", 52)),
    };
    assert_eq!(apply(&mut w, jira), Err(ActionError::NotATicket));
}

#[test]
fn the_new_actions_survive_the_wire_and_old_clients_need_no_new_fields() {
    let a = Action::CreateIssue {
        project: "PVT_1".into(),
        repo: "acme/app".into(),
        title: "T".into(),
        body: String::new(),
        status: None,
        created: None,
    };
    let json = serde_json::to_string(&a).unwrap();
    assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), a);
    // `body`, `status` and `created` may be left out by a client.
    let min = r#"{"CreateIssue":{"project":"P","repo":"a/b","title":"T"}}"#;
    assert!(serde_json::from_str::<Action>(min).is_ok());
    let field = r#"{"SetProjectField":{"item":"x","field_id":"F"}}"#;
    assert!(matches!(
        serde_json::from_str::<Action>(field).unwrap(),
        Action::SetProjectField { value: None, .. }
    ));
}
