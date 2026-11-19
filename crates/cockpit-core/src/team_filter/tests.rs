use super::*;
use crate::model::{Scope, TicketStatus};

fn team(id: &str) -> TeamRef {
    TeamRef::new(id, &format!("Team {id}"))
}

fn ws() -> Workspace {
    let mut ws = crate::demo::workspace();
    ws.items.clear();
    ws
}

fn ticket(ws: &mut Workspace, key: &str, team: Option<TeamRef>, scope: Scope) -> Item {
    let mut t = crate::demo::workspace().items[0].clone();
    t.id = key.to_lowercase();
    t.key = key.into();
    t.team = team;
    t.scope = scope;
    t.pr = None;
    t.branch = None;
    ws.items.push(t.clone());
    t
}

fn pull_request(ws: &mut Workspace, number: u32, branch: &str) -> Item {
    let mut p = crate::demo::workspace().items[1].clone();
    p.id = format!("pr{number}");
    p.scope = Scope::Team;
    p.branch = Some(branch.into());
    p.title = format!("PR {number}");
    if let Some(r) = p.pr.as_mut() {
        r.number = number;
    }
    ws.items.push(p.clone());
    p
}

fn choose(ws: &mut Workspace, ids: &[&str], none: bool) {
    ws.team_choice = TeamChoice {
        teams: ids.iter().map(|i| team(i)).collect(),
        include_no_team: none,
    };
}

#[test]
fn without_a_choice_everything_shows() {
    let mut w = ws();
    let a = ticket(&mut w, "APP-1", Some(team("a")), Scope::Team);
    let b = ticket(&mut w, "APP-2", None, Scope::Team);
    assert!(w.team_visible(&a) && w.team_visible(&b));
}

#[test]
fn tickets_of_other_teams_are_hidden() {
    let mut w = ws();
    let a = ticket(&mut w, "APP-1", Some(team("a")), Scope::Team);
    let b = ticket(&mut w, "APP-2", Some(team("b")), Scope::Team);
    choose(&mut w, &["a"], false);
    assert!(w.team_visible(&a));
    assert!(!w.team_visible(&b));
}

#[test]
fn several_teams_can_be_chosen() {
    let mut w = ws();
    let b = ticket(&mut w, "APP-2", Some(team("b")), Scope::Team);
    let c = ticket(&mut w, "APP-3", Some(team("c")), Scope::Team);
    choose(&mut w, &["a", "b"], false);
    assert!(w.team_visible(&b));
    assert!(!w.team_visible(&c));
}

#[test]
fn tickets_without_a_team_follow_the_option() {
    let mut w = ws();
    let none = ticket(&mut w, "APP-2", None, Scope::Team);
    choose(&mut w, &["a"], false);
    assert!(!w.team_visible(&none));
    choose(&mut w, &["a"], true);
    assert!(w.team_visible(&none));
}

#[test]
fn my_own_entries_always_show() {
    let mut w = ws();
    let mine = ticket(&mut w, "APP-9", Some(team("z")), Scope::Mine);
    choose(&mut w, &["a"], false);
    assert!(w.team_visible(&mine));
}

#[test]
fn a_pull_request_without_a_ticket_is_never_filtered() {
    let mut w = ws();
    ticket(&mut w, "APP-1", Some(team("b")), Scope::Team);
    let pr = pull_request(&mut w, 5, "fix/typo");
    choose(&mut w, &["a"], false);
    assert!(w.team_visible(&pr));
}

#[test]
fn a_pull_request_of_another_teams_ticket_is_hidden() {
    let mut w = ws();
    ticket(&mut w, "APP-1", Some(team("b")), Scope::Team);
    let pr = pull_request(&mut w, 5, "feature/app-1-thing");
    choose(&mut w, &["a"], false);
    assert!(!w.team_visible(&pr));
    choose(&mut w, &["a", "b"], false);
    assert!(w.team_visible(&pr));
}

#[test]
fn a_pull_request_is_shown_if_one_of_its_tickets_is() {
    let mut w = ws();
    ticket(&mut w, "APP-1", Some(team("b")), Scope::Team);
    ticket(&mut w, "APP-2", Some(team("a")), Scope::Team);
    let pr = pull_request(&mut w, 5, "feature/app-1-and-app-2");
    choose(&mut w, &["a"], false);
    assert!(w.team_visible(&pr));
}

#[test]
fn a_pull_request_the_ticket_carries_counts_as_linked() {
    let mut w = ws();
    let mut t = ticket(&mut w, "APP-1", Some(team("b")), Scope::Team);
    let pr = pull_request(&mut w, 5, "no-key");
    t.pr = pr.pr.clone();
    w.items[0] = t;
    choose(&mut w, &["a"], false);
    assert!(!w.team_visible(&pr));
}

#[test]
fn a_pull_request_of_a_ticket_without_a_team_follows_the_option() {
    let mut w = ws();
    ticket(&mut w, "APP-1", None, Scope::Team);
    let pr = pull_request(&mut w, 5, "feature/app-1");
    choose(&mut w, &["a"], false);
    assert!(!w.team_visible(&pr));
    choose(&mut w, &["a"], true);
    assert!(w.team_visible(&pr));
}

#[test]
fn teams_are_listed_once_by_name_and_keep_the_chosen_one() {
    let mut w = ws();
    ticket(
        &mut w,
        "APP-1",
        Some(TeamRef::new("b", "Beta")),
        Scope::Team,
    );
    ticket(
        &mut w,
        "APP-2",
        Some(TeamRef::new("b", "Beta")),
        Scope::Team,
    );
    ticket(
        &mut w,
        "APP-3",
        Some(TeamRef::new("a", "alpha")),
        Scope::Team,
    );
    w.team_choice.teams = vec![TeamRef::new("gone", "Aus")];
    let names: Vec<String> = w.teams().into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["alpha", "Aus", "Beta"]);
}

#[test]
fn a_choice_is_cleaned_and_only_changes_when_different() {
    let mut w = ws();
    let choice = TeamChoice {
        teams: vec![team("a"), team("a"), TeamRef::new("", "x")],
        include_no_team: false,
    };
    assert!(w.set_team_choice(choice.clone()));
    assert_eq!(w.team_choice.teams.len(), 1);
    assert!(!w.set_team_choice(choice));
    let _ = TicketStatus::Open;
}

#[test]
fn old_data_without_the_field_loads() {
    let json = serde_json::to_value(crate::demo::workspace()).unwrap();
    let mut json = json;
    json.as_object_mut().unwrap().remove("team_choice");
    let w: Workspace = serde_json::from_value(json).unwrap();
    assert!(!w.team_choice.is_active());
}
