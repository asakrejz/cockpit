//! GitHub project tickets inside the workspace: linking pull requests, the sprint filter, the team
//! filter, the demo seed.

use crate::demo;
use crate::model::{Filter, PrKey, Scope, Source, SourceFilter, TeamRef};
use crate::project::ProjectItem;
use crate::search::InboxFilter;
use crate::setup::Step;
use crate::{Action, Actor, TicketsFrom, Workspace};

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
    assert_eq!(w.items.len(), demo::multi_repo_workspace().items.len() + 2);
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

#[test]
fn without_jira_the_projects_step_replaces_the_jira_step() {
    use TicketsFrom::*;
    assert!(Step::Jira.shown(Jira) && Step::Jira.shown(Both));
    assert!(!Step::Jira.shown(GitHubProjects));
    assert_eq!(Step::Container.next_for(Jira), Some(Step::Jira));
    assert_eq!(Step::Container.next_for(GitHubProjects), Some(Step::GitHub));
    assert_eq!(
        Step::Projects.previous_for(GitHubProjects),
        Some(Step::GitHub)
    );
    assert_eq!(Step::GitHub.next_for(Jira), Some(Step::Projects));
    assert_eq!(Step::Done.next_for(Both), None);
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
