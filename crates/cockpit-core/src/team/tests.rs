use super::*;
use crate::{AgentOptions, Checks, demo};
use cockpit_i18n::Lang;

/// Without a plan step: the agent gets going right away.
fn direct() -> AgentOptions {
    AgentOptions {
        plan_first: false,
        ..AgentOptions::default()
    }
}

#[test]
fn a_plan_waiting_for_approval_is_stuck_with_me() {
    let mut w = demo::workspace();
    w.set_status("bau150", TicketStatus::InProgress);
    w.start_agent("BAU-150", AgentOptions::default()).unwrap();
    let s = stuck(&w);
    let entry = s.iter().find(|s| s.key == "BAU-150").unwrap();
    assert_eq!(entry.kind, StuckKind::PlanWaiting);
    assert_eq!(entry.who, Person::me());
    assert_eq!(entry.plan.as_deref(), Some("BAU-150"));
    // The waiting agent occupies the ticket.
    assert_eq!(member(&members(&w), "Agent").in_progress, 1);
    // After approval it runs – then nothing is stuck any more.
    w.report_agent(
        "BAU-150",
        crate::agent::AgentReport::Plan {
            summary: "Plan".into(),
            files: vec!["a.rs".into()],
            steps: vec!["Change it".into()],
        },
        0,
    )
    .unwrap();
    assert!(w.approve_plan("BAU-150"));
    assert!(stuck(&w).iter().all(|s| s.key != "BAU-150"));
}

fn member<'a>(list: &'a [Member], name: &str) -> &'a Member {
    list.iter()
        .find(|m| m.person.name_in(cockpit_i18n::Lang::De) == name)
        .unwrap()
}

#[test]
fn demo_members_are_grouped_by_person() {
    let list = members(&demo::workspace());
    let names: Vec<_> = list
        .iter()
        .map(|m| m.person.name_in(cockpit_i18n::Lang::De))
        .collect();
    assert_eq!(names, ["Du", "Agent", "Jonas", "Mira"]);

    let me = member(&list, "Du");
    assert_eq!(
        (me.open, me.in_progress, me.review, me.pull_requests),
        (2, 1, 1, 1)
    );
    assert_eq!(me.item_ids, ["bau142", "bau138", "bau150", "doc7", "pr12"]);
    // The agent has finished BAU-138, the review is waiting.
    let agent = member(&list, "Agent");
    assert_eq!((agent.in_progress, agent.review), (0, 1));
    assert_eq!(agent.item_ids, ["bau138"]);
    assert_eq!(member(&list, "Jonas").pull_requests, 1);
    assert_eq!(member(&list, "Mira").item_ids, ["pr85"]);
}

#[test]
fn me_and_agent_are_always_listed() {
    let list = members(&Workspace::default());
    assert_eq!(list.len(), 2);
    assert!(list.iter().all(|m| m.total() == 0));
    assert_eq!(list[0].load_level(), LoadLevel::Free);
    assert_eq!(list[0].load_percent(), 0);
}

#[test]
fn load_grows_with_open_entries() {
    let mut m = Member::new(Person::me());
    let mut levels = Vec::new();
    for _ in 0..=CAPACITY + 1 {
        levels.push((m.load_level(), m.load_percent()));
        m.open += 1;
    }
    assert_eq!(levels[0], (LoadLevel::Free, 0));
    assert_eq!(levels[1], (LoadLevel::Free, 25));
    assert_eq!(levels[2], (LoadLevel::Busy, 50));
    assert_eq!(levels[3], (LoadLevel::Busy, 75));
    assert_eq!(levels[CAPACITY], (LoadLevel::Full, 100));
    // Beyond the capacity the bar stays at 100.
    assert_eq!(levels[CAPACITY + 1], (LoadLevel::Full, 100));
}

#[test]
fn done_entries_do_not_count() {
    let mut w = demo::workspace();
    w.mark_done("bau150");
    w.set_status("bau142", TicketStatus::Done);
    let list = members(&w);
    assert_eq!(member(&list, "Du").item_ids, ["bau138", "doc7", "pr12"]);
}

#[test]
fn running_agent_counts_as_in_progress() {
    let mut w = demo::workspace();
    w.start_agent("BAU-150", direct()).unwrap();
    let list = members(&w);
    let agent = member(&list, "Agent");
    assert_eq!((agent.in_progress, agent.review), (1, 1));
    assert_eq!(agent.item_ids, ["bau138", "bau150"]);
    // When cancelled it no longer counts.
    w.cancel_agent("BAU-150");
    assert_eq!(member(&members(&w), "Agent").item_ids, ["bau138"]);
}

#[test]
fn demo_stuck_entries_in_order_of_urgency() {
    let list = stuck(&demo::workspace());
    let found: Vec<_> = list.iter().map(|s| (s.key.as_str(), s.kind)).collect();
    assert_eq!(
        found,
        [
            ("#85", StuckKind::ChecksFailed),
            ("#12", StuckKind::ChecksFailed),
            ("BAU-138", StuckKind::AgentWaiting),
            ("#87", StuckKind::ReviewWaiting),
            ("BAU-142", StuckKind::NoBranch),
        ]
    );
    // BAU-150 is only open and not stuck.
    assert!(list.iter().all(|s| s.key != "BAU-150"));
}

#[test]
fn stuck_entries_name_reason_and_target() {
    let list = stuck(&demo::workspace());
    let by_key = |k: &str| list.iter().find(|s| s.key == k).unwrap();

    let failed = by_key("#85");
    assert_eq!(failed.who.name, "Mira");
    assert!(failed.reason.render(Lang::De).contains("Mira"));
    assert_eq!(failed.review.as_ref().map(|k| k.number), Some(85));

    let agent = by_key("BAU-138");
    assert_eq!(agent.who, Person::me());
    assert!(agent.reason.render(Lang::De).contains("#91"));
    assert_eq!(agent.review.as_ref().map(|k| k.number), Some(91));

    let review = by_key("#87");
    assert!(review.reason.render(Lang::De).starts_with("Jonas wartet"));

    let branch = by_key("BAU-142");
    assert_eq!(branch.review, None);
    assert!(branch.reason.render(Lang::De).contains("keinen Branch"));
}

#[test]
fn creating_a_branch_unblocks_the_ticket() {
    let mut w = demo::workspace();
    w.create_branch("bau142");
    assert!(stuck(&w).iter().all(|s| s.key != "BAU-142"));
}

#[test]
fn sending_the_review_unblocks_the_pull_request() {
    let mut w = demo::workspace();
    for s in ["a", "b", "c"] {
        w.decide(87, s, Some(crate::Decision::Approved));
    }
    w.send_review(87, "", 0).unwrap();
    assert!(stuck(&w).iter().all(|s| s.key != "#87"));
    // For Jonas nothing is open any more; whoever has nothing does not appear any more.
    assert!(
        members(&w)
            .iter()
            .all(|m| m.person.name != "Jonas" || m.total() == 0)
    );
}

#[test]
fn a_finished_agent_run_is_not_stuck_before_its_pull_request_exists() {
    let mut w = demo::workspace();
    w.start_agent("BAU-150", direct()).unwrap();
    // While it is still running, nothing is stuck – even if the branch only comes into being at the end.
    assert!(stuck(&w).iter().all(|s| s.key != "BAU-150"));
    for _ in 0..crate::agent::STEPS {
        crate::test_support::advance(&mut w);
    }
    // The pushed branch waits for your approval of the pull request; that is not a block.
    assert!(w.run("BAU-150").unwrap().is_done());
    assert!(stuck(&w).iter().all(|s| s.key != "BAU-150"));
}

#[test]
fn failed_checks_win_over_other_reasons() {
    let mut w = demo::workspace();
    // PR 87 is waiting for review – if the checks fail, that is the reason.
    w.items
        .iter_mut()
        .find(|i| i.id == "pr87")
        .and_then(|i| i.pr.as_mut())
        .unwrap()
        .checks = Checks::Failed;
    let list = stuck(&w);
    let entry = list.iter().find(|s| s.key == "#87").unwrap();
    assert_eq!(entry.kind, StuckKind::ChecksFailed);
    assert_eq!(list.iter().filter(|s| s.key == "#87").count(), 1);
}

#[test]
fn drafts_and_running_checks_do_not_hang() {
    let mut w = demo::workspace();
    let pr = w
        .items
        .iter_mut()
        .find(|i| i.id == "pr87")
        .and_then(|i| i.pr.as_mut())
        .unwrap();
    pr.draft = true;
    assert!(stuck(&w).iter().all(|s| s.key != "#87"));
    let pr = w
        .items
        .iter_mut()
        .find(|i| i.id == "pr87")
        .and_then(|i| i.pr.as_mut())
        .unwrap();
    pr.draft = false;
    pr.checks = Checks::Running;
    assert!(stuck(&w).iter().all(|s| s.key != "#87"));
}

#[test]
fn state_tone_for_tickets_and_pull_requests() {
    let w = demo::workspace();
    let tone = |id: &str| state_tone(w.item(id).unwrap());
    assert_eq!(tone("bau150"), Tone::Gray);
    assert_eq!(tone("bau142"), Tone::Blue);
    assert_eq!(tone("bau138"), Tone::Orange);
    assert_eq!(tone("pr85"), Tone::Red);
    assert_eq!(tone("pr87"), Tone::Orange);
}

#[test]
fn nothing_hangs_in_an_empty_workspace() {
    assert!(stuck(&Workspace::default()).is_empty());
}

#[test]
fn own_entries_read_naturally() {
    let mut w = demo::workspace();
    w.items.iter_mut().find(|i| i.id == "pr85").unwrap().owner = Person::me();
    let list = stuck(&w);
    let entry = list.iter().find(|s| s.key == "#85").unwrap();
    assert!(
        entry
            .reason
            .render(Lang::De)
            .contains("Du kommst nicht weiter")
    );
}

#[test]
fn a_waiting_question_is_stuck_with_me() {
    let mut w = demo::workspace();
    w.start_agent("BAU-142", direct()).unwrap();
    crate::test_support::advance(&mut w);
    crate::test_support::advance(&mut w);
    crate::test_support::question(&mut w, "BAU-142", "Remove it or mark it as deprecated?");
    let s = stuck(&w);
    let entry = s.iter().find(|s| s.key == "BAU-142").unwrap();
    assert_eq!(entry.kind, StuckKind::QuestionWaiting);
    assert_eq!(entry.kind, StuckKind::QuestionWaiting);
    assert_eq!(entry.who, Person::me());
    assert_eq!(entry.question.as_deref(), Some("BAU-142"));
    assert!(entry.plan.is_none());
    assert!(
        entry.reason.render(Lang::De).contains("deprecated"),
        "{}",
        entry.reason.render(Lang::De)
    );
    assert_eq!(w.waiting_agents(), 1, "counts in the sidebar under agents");
    // A question is more urgent than a waiting plan.
    assert!(StuckKind::QuestionWaiting < StuckKind::PlanWaiting);
    // After the answer nothing is stuck any more.
    assert!(w.answer_agent("BAU-142", "Entfernen"));
    assert!(stuck(&w).iter().all(|s| s.key != "BAU-142"));
    assert_eq!(w.waiting_agents(), 0);
}

// ---------- The chosen person ----------

fn spread() -> Workspace {
    let mut w = demo::workspace();
    w.set_status("bau150", TicketStatus::InProgress);
    w
}

#[test]
fn a_chosen_person_narrows_stuck_entries_and_entries_to_theirs() {
    let w = spread();
    let list = members(&w);
    let jonas = chosen(&list, Some("github:Jonas"));
    assert!(jonas.is_some());
    let ids = entry_ids(&w, &list, jonas);
    assert_eq!(ids, jonas.unwrap().item_ids);
    assert!(
        ids.iter()
            .all(|id| w.item(id).unwrap().owner.name == "Jonas"),
        "only the tickets of the chosen person"
    );
    for s in stuck_of(stuck(&w), jonas) {
        assert!(ids.contains(&s.item_id), "{} belongs to Jonas", s.key);
    }
}

#[test]
fn nobody_or_an_unknown_name_means_everybody() {
    let w = spread();
    let list = members(&w);
    assert!(chosen(&list, None).is_none());
    assert!(chosen(&list, Some("Nobody Here")).is_none());
    let all = entry_ids(&w, &list, None);
    // Every open entry once, whoever holds it.
    let open = w
        .items
        .iter()
        .filter(|i| !i.read && i.status != TicketStatus::Done)
        .count();
    assert_eq!(all.len(), open);
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len(), "no entry twice");
    assert_eq!(stuck_of(stuck(&w), None), stuck(&w));
    // The entries of each person are part of everybody's.
    for m in &list {
        assert!(
            m.item_ids.iter().all(|id| all.contains(id)),
            "{}",
            m.person.name
        );
    }
}

#[test]
fn the_team_choice_limits_members_and_stuck_but_keeps_me_and_the_agent() {
    use crate::TeamRef;
    let mut w = demo::multi_repo_workspace();
    let all = members(&w);
    w.team_choice.teams = vec![TeamRef::new("team-design", "Design System")];
    let list = members(&w);
    let names: Vec<_> = list
        .iter()
        .map(|m| m.person.name_in(cockpit_i18n::Lang::De))
        .collect();
    // Jonas only owns a ticket of the Plattform team (his pull request has no ticket, so it stays).
    assert!(names.contains(&"Jonas".to_string()));
    assert_eq!(&names[..2], ["Du", "Agent"]);
    // Tickets of other teams are not counted anywhere.
    let ids: Vec<&String> = list.iter().flat_map(|m| &m.item_ids).collect();
    assert!(!ids.iter().any(|id| id.as_str() == "web21"));
    // Without a choice they were.
    assert!(
        all.iter()
            .flat_map(|m| &m.item_ids)
            .any(|id| id.as_str() == "web21")
    );
}

#[test]
fn the_team_choice_hides_the_stuck_entries_of_other_teams() {
    use crate::TeamRef;
    let mut w = demo::multi_repo_workspace();
    // A ticket of another team that is stuck: in progress without a branch.
    w.set_status("web21", TicketStatus::InProgress);
    assert!(stuck(&w).iter().any(|s| s.key == "WEB-21"));
    w.team_choice.teams = vec![TeamRef::new("team-design", "Design System")];
    assert!(stuck(&w).iter().all(|s| s.key != "WEB-21"));
    w.team_choice.teams = vec![TeamRef::new("team-web", "Web")];
    assert!(stuck(&w).iter().any(|s| s.key == "WEB-21"));
}

// ---------- People per source ----------

use crate::{PeopleSource, Source, TeamRef};

/// Jira: Du, "Jonas Berg" and "Mira". GitHub: Du, "Jonas" and "Mira". Nothing linked.
fn two_sources() -> Workspace {
    demo::seed()
}

fn keys(list: &[Member]) -> Vec<&str> {
    list.iter().map(|m| m.key.as_str()).collect()
}

#[test]
fn the_same_name_in_both_sources_stays_two_people() {
    let list = members(&two_sources());
    let mira: Vec<_> = list.iter().filter(|m| m.person.name == "Mira").collect();
    assert_eq!(mira.len(), 2);
    assert_ne!(mira[0].key, mira[1].key);
    assert_eq!(mira[0].sides.len(), 1);
    // Jonas Berg (Jira) and Jonas (GitHub) are not merged by guess.
    assert!(keys(&list).contains(&"jira:Jonas Berg"));
    assert!(keys(&list).contains(&"github:Jonas"));
}

#[test]
fn the_source_filter_limits_people_and_load() {
    let mut w = two_sources();
    w.people_source = PeopleSource::Jira;
    let list = members(&w);
    assert!(!keys(&list).contains(&"github:Jonas"));
    assert!(keys(&list).contains(&"jira:Jonas Berg"));
    // Du keeps only Jira entries: the three tickets, no pull request.
    assert_eq!(member(&list, "Du").pull_requests, 0);
    assert_eq!(
        member(&list, "Du").open + member(&list, "Du").in_progress,
        3
    );

    w.people_source = PeopleSource::GitHub;
    let list = members(&w);
    assert!(!keys(&list).contains(&"jira:Jonas Berg"));
    assert!(keys(&list).contains(&"github:Jonas"));
    let me = member(&list, "Du");
    assert_eq!(me.pull_requests, 1);
    assert!(me.item_ids.iter().all(|id| {
        w.item(id)
            .is_some_and(|i| PeopleSource::GitHub.allows(i.source))
    }));
}

#[test]
fn github_projects_tickets_count_as_github() {
    let mut w = two_sources();
    let issue = w
        .items
        .iter()
        .find(|i| i.source == Source::GitHubProject)
        .expect("the seed has a project ticket")
        .id
        .clone();
    w.people_source = PeopleSource::GitHub;
    let on_github: Vec<String> = members(&w).into_iter().flat_map(|m| m.item_ids).collect();
    assert!(on_github.contains(&issue));
    w.people_source = PeopleSource::Jira;
    let on_jira: Vec<String> = members(&w).into_iter().flat_map(|m| m.item_ids).collect();
    assert!(!on_jira.contains(&issue));
}

#[test]
fn stuck_follows_the_source_filter() {
    let mut w = two_sources();
    let all = stuck(&w);
    w.people_source = PeopleSource::Jira;
    let jira = stuck(&w);
    w.people_source = PeopleSource::GitHub;
    let github = stuck(&w);
    assert!(jira.len() <= all.len() && github.len() <= all.len());
    assert_eq!(jira.len() + github.len(), all.len());
    assert!(
        jira.iter()
            .all(|s| w.item(&s.item_id).unwrap().source == Source::Jira)
    );
    assert!(
        github
            .iter()
            .all(|s| w.item(&s.item_id).unwrap().source != Source::Jira)
    );
}

#[test]
fn sections_group_by_source_under_all_and_are_flat_otherwise() {
    let mut w = two_sources();
    let list = members(&w);
    let secs = sections(&w, &list);
    let groups: Vec<_> = secs.iter().map(|(g, _)| *g).collect();
    assert_eq!(
        groups,
        [Some(Group::Both), Some(Group::Jira), Some(Group::GitHub)]
    );
    let names = |g| -> Vec<String> {
        secs.iter()
            .find(|(x, _)| *x == Some(g))
            .unwrap()
            .1
            .iter()
            .map(|m| m.key.clone())
            .collect()
    };
    assert_eq!(names(Group::Both), ["Du", "Agent"]);
    assert!(names(Group::Jira).contains(&"jira:Jonas Berg".to_string()));
    assert!(names(Group::GitHub).contains(&"github:Jonas".to_string()));

    w.people_source = PeopleSource::Jira;
    let list = members(&w);
    let secs = sections(&w, &list);
    assert_eq!(secs.len(), 1);
    assert_eq!(secs[0].0, None);
    assert_eq!(secs[0].1.len(), list.len());
}

#[test]
fn a_link_joins_two_people_into_one_member_with_both_sources() {
    let mut w = two_sources();
    assert!(w.link_people("Jonas Berg", "Jonas"));
    let list = members(&w);
    let jonas: Vec<_> = list.iter().filter(|m| m.key.starts_with("link:")).collect();
    assert_eq!(jonas.len(), 1);
    let j = jonas[0];
    assert_eq!(j.person.name, "Jonas Berg");
    assert_eq!(j.github.as_deref(), Some("Jonas"));
    assert_eq!(j.sides.len(), 2);
    assert_eq!(j.group(), Group::Both);
    assert_eq!(j.pull_requests, 2);
    assert_eq!(j.in_progress, 1);
    // One source only: the member shows just that part.
    w.people_source = PeopleSource::GitHub;
    let list = members(&w);
    let j = list.iter().find(|m| m.key == "link:Jonas Berg").unwrap();
    assert_eq!((j.pull_requests, j.total()), (2, 2));
    // The other Mira stays untouched.
    assert_eq!(list.iter().filter(|m| m.person.name == "Mira").count(), 1);
}

#[test]
fn unlinking_splits_the_member_again() {
    let mut w = two_sources();
    w.link_people("Jonas Berg", "Jonas");
    assert!(w.unlink_people("Jonas Berg"));
    assert!(!w.unlink_people("Jonas Berg"));
    let list = members(&w);
    assert!(keys(&list).contains(&"jira:Jonas Berg") && keys(&list).contains(&"github:Jonas"));
}

#[test]
fn a_link_is_one_to_one_and_never_for_me_or_the_agent() {
    let mut w = Workspace::default();
    assert!(w.link_people("Jonas Berg", "jonas"));
    assert!(!w.link_people("Jonas Berg", "jonas"));
    // A new link of either side replaces the old one.
    assert!(w.link_people("Jonas Berg", "jb"));
    assert!(w.link_people("Jonas B.", "jb"));
    assert_eq!(w.person_links.len(), 1);
    assert_eq!(w.person_links[0].jira, "Jonas B.");
    for (a, b) in [
        ("Du", "x"),
        ("x", "Du"),
        ("Agent", "x"),
        ("", "x"),
        (" ", "x"),
        ("x", ""),
    ] {
        assert!(!w.link_people(a, b), "{a:?} {b:?}");
    }
}

#[test]
fn me_is_one_member_with_both_sources_without_a_link() {
    let list = members(&two_sources());
    let me = member(&list, "Du");
    assert_eq!(me.group(), Group::Both);
    assert!(me.sides.contains(&Side::Jira) && me.sides.contains(&Side::GitHub));
    assert!(me.unlinked_side().is_none() && !me.is_linked());
    assert_eq!(list.iter().filter(|m| m.person == Person::me()).count(), 1);
}

#[test]
fn the_team_filter_still_applies_per_source() {
    let mut w = two_sources();
    let team = TeamRef::new("a", "Team a");
    for i in w.items.iter_mut().filter(|i| i.id == "bau160") {
        i.team = Some(team.clone());
    }
    w.team_choice = crate::TeamChoice {
        teams: vec![TeamRef::new("zzz", "Other")],
        include_no_team: false,
    };
    // Jonas Berg's ticket is of another team: hidden in every source view.
    for source in PeopleSource::ALL {
        w.people_source = source;
        assert!(!keys(&members(&w)).contains(&"jira:Jonas Berg"));
    }
}

#[test]
fn chosen_finds_the_member_by_key() {
    let list = members(&two_sources());
    assert_eq!(
        chosen(&list, Some("github:Mira")).unwrap().key,
        "github:Mira"
    );
    assert_eq!(chosen(&list, Some("jira:Mira")).unwrap().key, "jira:Mira");
    assert!(chosen(&list, Some("Mira")).is_none());
    assert!(chosen(&list, None).is_none());
}

#[test]
fn old_saved_states_load_without_the_new_fields() {
    let w: Workspace = serde_json::from_str("{}").unwrap();
    assert_eq!(w.people_source, PeopleSource::All);
    assert!(w.person_links.is_empty());
    let json = serde_json::to_string(&Workspace::default()).unwrap();
    assert!(!json.contains("person_links") && !json.contains("people_source"));
}

#[test]
fn link_candidates_are_the_unlinked_people_of_the_other_source() {
    use crate::team::Side;
    let mut w = two_sources();
    let github: Vec<_> = w
        .link_candidates(Side::Jira)
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(github.contains(&"Jonas".to_string()));
    assert!(!github.contains(&"Du".to_string()) && !github.contains(&"Jonas Berg".to_string()));
    let jira: Vec<_> = w
        .link_candidates(Side::GitHub)
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(jira.contains(&"Jonas Berg".to_string()));
    // A person that is linked is no candidate any more.
    w.link_people("Jonas Berg", "Jonas");
    assert!(
        !w.link_candidates(Side::Jira)
            .iter()
            .any(|p| p.name == "Jonas")
    );
    assert!(
        !w.link_candidates(Side::GitHub)
            .iter()
            .any(|p| p.name == "Jonas Berg")
    );
}
