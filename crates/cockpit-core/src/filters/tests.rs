use super::*;

#[test]
fn every_filter_has_a_unique_id_and_a_valid_default() {
    let mut f = crate::InboxFilter::default();
    for (n, def) in FILTERS.iter().enumerate() {
        assert!(
            FILTERS[n + 1..].iter().all(|o| o.id != def.id),
            "{}",
            def.id
        );
        assert_eq!(
            f.get(def.id),
            def.default,
            "{} starts at its default",
            def.id
        );
        assert!(f.set(def.id, def.default), "{} accepts its default", def.id);
    }
    assert!(f.is_default());
}

#[test]
fn fixed_choices_contain_the_default_or_the_all_choice() {
    for def in FILTERS {
        if let Choices::Fixed(list) = def.choices {
            assert!(
                def.default.is_empty() && def.has_all || list.contains(&def.default),
                "{}",
                def.id
            );
        }
    }
}

#[test]
fn ids_round_trip() {
    for id in Ownership::IDS {
        assert_eq!(Ownership::from_id(id).unwrap().id(), *id);
    }
    for id in SprintView::IDS {
        assert_eq!(SprintView::from_id(id).unwrap().id(), *id);
    }
    assert_eq!(Ownership::from_id("x"), None);
    assert_eq!(SprintView::from_id("x"), None);
}

#[test]
fn filter_def_finds_by_id() {
    assert_eq!(filter_def("sprint").unwrap().default, "current");
    assert!(filter_def("nope").is_none());
}

// ---------- The filters on a workspace ----------

use crate::{
    Filter, InboxFilter, Item, Kind, Scope, Source, SourceFilter, Sprint, SprintState,
    TicketStatus, Workspace, demo,
};

const NOW: u64 = 1_715_688_000_000; // 2024-05-14
const DAY_MS: u64 = 24 * 3600 * 1000;

fn ticket(key: &str, sprint: Option<SprintState>, status: TicketStatus, age_days: u64) -> Item {
    let mut item = demo::workspace().item_by_key("BAU-142").unwrap().clone();
    item.id = key.to_lowercase().replace('-', "");
    item.key = key.into();
    item.title = format!("Ticket {key}");
    item.status = status;
    item.comments.clear();
    item.sprint = sprint.map(|state| Sprint {
        name: "Sprint 7".into(),
        state,
        start: String::new(),
        end: String::new(),
    });
    item.updated_ms = Some(NOW - age_days * DAY_MS);
    item
}

fn pull(nr: u32, title: &str, branch: &str, mine: bool) -> Item {
    let mut item = demo::workspace().item_by_key("#87").unwrap().clone();
    item.id = format!("gh-acme-app-{nr}");
    item.key = format!("#{nr}");
    item.title = title.into();
    item.branch = Some(branch.into());
    item.repo = Some("acme/app".into());
    item.scope = if mine { Scope::Mine } else { Scope::Team };
    // Colleagues' pull requests carry no reason for attention.
    item.reason = crate::ReasonKind::Comment;
    item.comments.clear();
    if let Some(pr) = item.pr.as_mut() {
        pr.number = nr;
        pr.repo = "acme/app".into();
        pr.review_requested = false;
        pr.mine = mine;
        pr.checks = crate::Checks::Passed;
    }
    item
}

fn workspace(items: Vec<Item>) -> Workspace {
    Workspace {
        items,
        ..Workspace::default()
    }
}

fn keys(ws: &Workspace, by: &InboxFilter) -> Vec<String> {
    ws.inbox(Filter::All, SourceFilter::All, "", NOW, by)
        .iter()
        .map(|i| i.key.clone())
        .collect()
}

fn all() -> InboxFilter {
    InboxFilter::default()
}

fn mine() -> InboxFilter {
    InboxFilter {
        ownership: Ownership::Mine,
        ..Default::default()
    }
}

#[test]
fn the_start_state_is_everybody_current_sprint_and_done_hidden() {
    let by = InboxFilter::default();
    assert_eq!(by.ownership, Ownership::All);
    assert_eq!(by.person, None, "assignee: everybody");
    assert_eq!(by.sprint, SprintView::Current);
    assert!(by.hide_done);
    assert!(by.is_default());
    assert!(!mine().is_default());
}

#[test]
fn ownership_mine_is_the_attention_list_and_all_adds_the_quiet_pull_requests() {
    let ws = workspace(vec![
        pull(1, "Mine", "feature/x", true),
        pull(2, "Colleague", "feature/y", false),
    ]);
    // Neither raises attention on its own here (reason „Comment“, no failure) …
    assert!(ws.attention(Filter::All, NOW).is_empty());
    assert!(keys(&ws, &mine()).is_empty());
    // … but „Alle“ lists them, and the unread count never includes them.
    assert_eq!(keys(&ws, &all()), ["#1", "#2"]);
    assert_eq!(ws.unread(Filter::All, NOW), 0);
    // The scope tabs still apply: „Team“ holds only the colleague's.
    let team = ws.inbox(Filter::Team, SourceFilter::All, "", NOW, &all());
    assert_eq!(team.len(), 1);
    assert_eq!(team[0].key, "#2");
}

#[test]
fn snoozed_and_foreign_entries_do_not_mix() {
    let mut ws = workspace(vec![pull(2, "Colleague", "feature/y", false)]);
    assert!(ws.snooze("gh-acme-app-2", None));
    assert!(keys(&ws, &all()).is_empty(), "a snoozed entry stays hidden");
    let snoozed = ws.inbox(Filter::Snoozed, SourceFilter::All, "", NOW, &all());
    assert_eq!(snoozed.len(), 1);
}

#[test]
fn the_project_comes_from_the_key_of_ticket_branch_or_title() {
    let ws = workspace(vec![
        ticket("BAU-1", Some(SprintState::Active), TicketStatus::Open, 1),
        ticket("WEB-2", Some(SprintState::Active), TicketStatus::Open, 1),
        pull(10, "Fix focus", "feature/bau-142-focus", false),
        pull(11, "WEB-9: header", "topic", false),
        pull(12, "Refactor", "refactor", false),
    ]);
    let project = |p: &str| InboxFilter {
        project: Some(p.into()),
        ..all()
    };
    assert_eq!(keys(&ws, &project("BAU")), ["BAU-1", "#10"]);
    assert_eq!(keys(&ws, &project("WEB")), ["WEB-2", "#11"]);
    assert_eq!(
        keys(&ws, &project(NO_TICKET)),
        ["#12"],
        "only pull requests without a key"
    );
    let choices = ws.filter_choices("project");
    assert_eq!(
        choices,
        [
            ("BAU".to_string(), "BAU".to_string()),
            ("WEB".to_string(), "WEB".to_string()),
            (NO_TICKET.to_string(), NO_TICKET.to_string()),
        ]
    );
}

#[test]
fn the_sprint_filter_and_done_toggle() {
    let ws = workspace(vec![
        ticket(
            "BAU-1",
            Some(SprintState::Active),
            TicketStatus::InProgress,
            30,
        ),
        ticket("BAU-2", Some(SprintState::Closed), TicketStatus::Open, 3),
        ticket("BAU-3", None, TicketStatus::Open, 40),
        ticket("BAU-4", Some(SprintState::Future), TicketStatus::Open, 20),
        ticket("BAU-5", Some(SprintState::Active), TicketStatus::Done, 1),
    ]);
    let by = |sprint, hide_done| InboxFilter {
        sprint,
        hide_done,
        ..all()
    };
    assert_eq!(keys(&ws, &by(SprintView::Current, true)), ["BAU-1"]);
    assert_eq!(
        keys(&ws, &by(SprintView::Current, false)),
        ["BAU-1", "BAU-5"]
    );
    assert_eq!(keys(&ws, &by(SprintView::Backlog, true)), ["BAU-3"]);
    assert_eq!(
        keys(&ws, &by(SprintView::Recent, true)),
        ["BAU-2"],
        "14 days, no done"
    );
    assert_eq!(
        keys(&ws, &by(SprintView::AllOpen, true)),
        ["BAU-1", "BAU-2", "BAU-3", "BAU-4"]
    );
    assert_eq!(keys(&ws, &by(SprintView::AllOpen, false)).len(), 5);
}

#[test]
fn the_sprint_filter_leaves_pull_requests_alone() {
    let ws = workspace(vec![
        ticket("BAU-1", None, TicketStatus::Open, 1),
        ticket("BAU-2", Some(SprintState::Active), TicketStatus::Open, 1),
        pull(5, "Colleague", "x", false),
    ]);
    // Tickets have sprint data (BAU-2), so "Current sprint" is strict – pull requests stay.
    assert_eq!(keys(&ws, &all()), ["BAU-2", "#5"]);
}

#[test]
fn without_any_sprint_data_the_current_sprint_shows_all_open_tickets() {
    let ws = workspace(vec![
        ticket("BAU-1", None, TicketStatus::Open, 1),
        ticket("BAU-2", None, TicketStatus::Done, 1),
    ]);
    assert!(!ws.has_sprint_data());
    assert_eq!(keys(&ws, &all()), ["BAU-1"], "open ones, done stay hidden");
}

#[test]
fn filters_combine() {
    let ws = workspace(vec![
        ticket("BAU-1", Some(SprintState::Active), TicketStatus::Open, 1),
        pull(10, "Fix", "feature/bau-7", false),
        pull(11, "Other", "feature/web-7", false),
    ]);
    let by = InboxFilter {
        project: Some("BAU".into()),
        repo: Some("acme/app".into()),
        ..all()
    };
    assert_eq!(keys(&ws, &by), ["#10"], "a repo drops tickets");
    let by = InboxFilter {
        person: Some("Nobody".into()),
        ..by
    };
    assert!(keys(&ws, &by).is_empty());
}

#[test]
fn values_read_and_write_by_id() {
    let mut by = InboxFilter::default();
    assert!(by.set("ownership", "all"));
    assert!(by.set("sprint", "backlog"));
    assert!(by.set("done", "off"));
    assert!(by.set("status", "review"));
    assert!(by.set("project", "BAU"));
    assert!(by.set("repo", "a/b"));
    assert_eq!(by.get("ownership"), "all");
    assert_eq!(by.get("sprint"), "backlog");
    assert_eq!(by.get("done"), "off");
    assert_eq!(by.get("status"), "review");
    assert_eq!(by.get("project"), "BAU");
    assert!(!by.set("sprint", "nonsense"));
    assert!(!by.set("status", "nonsense"));
    assert!(!by.set("nope", "x"));
    assert_eq!(
        by.get("sprint"),
        "backlog",
        "a refused value changes nothing"
    );
    assert!(by.set("project", ""));
    assert_eq!(by.project, None);
}

#[test]
fn choices_come_from_the_definitions_or_the_data() {
    let ws = workspace(vec![
        ticket("BAU-1", None, TicketStatus::Open, 1),
        pull(1, "x", "y", true),
    ]);
    assert_eq!(ws.filter_choices("sprint").len(), SprintView::IDS.len());
    assert_eq!(
        ws.filter_choices("repo"),
        [("acme/app".to_string(), "acme/app".to_string())]
    );
    assert!(ws.filter_choices("unknown").is_empty());
    assert_eq!(Kind::PullRequest, ws.items[1].kind);
    assert_eq!(Source::GitHub, ws.items[1].source);
}

#[test]
fn the_review_list_follows_the_ownership_and_keeps_the_open_review() {
    let mut ws = demo::multi_repo_workspace();
    let all_reviews = ws.reviews.len();
    let mine = ws.reviews_for(Ownership::Mine, None).len();
    assert!(mine < all_reviews, "colleagues' pull requests are left out");
    assert_eq!(ws.reviews_for(Ownership::All, None).len(), all_reviews);
    let foreign = ws
        .reviews
        .iter()
        .find(|r| !ws.review_is_mine(r))
        .unwrap()
        .key();
    let kept = ws.reviews_for(Ownership::Mine, Some(&foreign));
    assert_eq!(kept.len(), mine + 1);
    // A review without an inbox entry counts as mine.
    ws.items.clear();
    assert_eq!(ws.reviews_for(Ownership::Mine, None).len(), all_reviews);
}

// ---------- Assignee, reporter, watching, type, priority ----------

fn people_workspace() -> Workspace {
    let mk = |key: &str, owner: &str, reporter: &str, kind: &str, p: crate::Priority| {
        let mut t = ticket(key, Some(SprintState::Active), TicketStatus::Open, 1);
        t.owner = if owner.is_empty() {
            crate::Person::nobody()
        } else {
            crate::Person::new(owner, "XX")
        };
        t.scope = Scope::Team;
        t.reporter = Some(reporter.into());
        t.issue_type = Some(kind.into());
        t.priority = p;
        t
    };
    let mut watched = mk("BAU-3", "Mira", "Jonas", "Task", crate::Priority::Low);
    watched.watching = true;
    workspace(vec![
        mk("BAU-1", "Lena", "Mira", "Bug", crate::Priority::High),
        mk("BAU-2", "", "Lena", "Story", crate::Priority::Medium),
        watched,
        pull(9, "A pull request", "topic", false),
    ])
}

fn with(id: &str, value: &str) -> InboxFilter {
    let mut by = InboxFilter::default();
    assert!(by.set(id, value));
    by
}

#[test]
fn the_start_state_lists_the_tickets_of_everybody() {
    let ws = people_workspace();
    assert_eq!(
        keys(&ws, &InboxFilter::default()),
        ["BAU-1", "BAU-2", "BAU-3", "#9"]
    );
}

#[test]
fn the_assignee_filter_knows_people_and_nobody() {
    let ws = people_workspace();
    assert_eq!(keys(&ws, &with("person", "Lena")), ["BAU-1"]);
    assert_eq!(keys(&ws, &with("person", UNASSIGNED)), ["BAU-2"]);
    let choices = ws.filter_choices("person");
    assert_eq!(choices[1].0, UNASSIGNED, "the unassigned choice is offered");
    assert!(choices.iter().any(|(v, _)| v == "Lena"));
}

#[test]
fn the_reporter_type_priority_and_watched_filters_narrow_tickets() {
    let ws = people_workspace();
    assert_eq!(keys(&ws, &with("reporter", "Mira")), ["BAU-1"]);
    assert_eq!(
        keys(&ws, &with("type", "story")),
        ["BAU-2"],
        "no regard to case"
    );
    assert_eq!(keys(&ws, &with("priority", "niedrig")), ["BAU-3"]);
    assert_eq!(keys(&ws, &with("watched", "on")), ["BAU-3"]);
    // Pull requests carry none of these fields: they drop out as soon as one is chosen.
    for id in ["reporter", "type", "watched"] {
        let value = if id == "watched" { "on" } else { "x" };
        assert!(
            !keys(&ws, &with(id, value)).contains(&"#9".to_string()),
            "{id}"
        );
    }
    let reporters: Vec<_> = ws
        .filter_choices("reporter")
        .into_iter()
        .map(|(v, _)| v)
        .collect();
    assert_eq!(reporters, [ASSIGNED_TO_ME, "Jonas", "Lena", "Mira"]);
    let types: Vec<_> = ws
        .filter_choices("type")
        .into_iter()
        .map(|(v, _)| v)
        .collect();
    assert_eq!(types, ["Bug", "Story", "Task"]);
}

#[test]
fn the_new_filters_reset_and_round_trip() {
    let mut by = InboxFilter::default();
    for (id, value) in [
        ("reporter", "Mira"),
        ("type", "Bug"),
        ("priority", "hoch"),
        ("watched", "on"),
    ] {
        assert!(by.set(id, value));
        assert_eq!(by.get(id), value);
        assert!(by.active().contains(&id), "{id} is active");
        assert!(by.reset(id));
        assert!(by.is_default(), "{id} is back at its start");
    }
    assert!(!by.set("priority", "nope"));
    assert!(!by.set("watched", "maybe"));
}

#[test]
fn an_entry_the_filters_hide_is_found_by_hidden_by_filter() {
    let ws = people_workspace();
    let by = with("person", "Lena");
    assert!(ws.hidden_by_filter("bau2", Filter::All, "", NOW, &by));
    assert!(!ws.hidden_by_filter("bau1", Filter::All, "", NOW, &by));
    assert!(!ws.hidden_by_filter("gibt-es-nicht", Filter::All, "", NOW, &by));
}

#[test]
fn resetting_for_a_hidden_entry_really_shows_it() {
    let mut ws = people_workspace();
    // A ticket without a sprint: the start values themselves hide it.
    let mut backlog = ticket("BAU-7", None, TicketStatus::Open, 1);
    backlog.scope = Scope::Team;
    ws.items.push(backlog);
    let narrowed = with("person", "Lena");
    assert!(ws.hidden_by_filter("bau7", Filter::All, "", NOW, &narrowed));
    let by = ws.filters_revealing("bau7", Filter::All, NOW);
    assert!(!ws.hidden_by_filter("bau7", Filter::All, "", NOW, &by));
    // An entry the start values show gets exactly the start values.
    assert_eq!(
        ws.filters_revealing("bau1", Filter::All, NOW),
        InboxFilter::default()
    );
}
