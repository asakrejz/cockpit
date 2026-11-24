use super::*;
use crate::model::{Kind, PrKey, Scope, TeamRef};
use crate::project::ProjectItem;
use crate::team_filter::TeamChoice;

/// The keys a pull request in `acme/app` names in these texts.
fn named(branch: &str, title: &str, description: &str) -> Vec<String> {
    PrTexts {
        repo: "acme/app",
        branch,
        title,
        description,
    }
    .ticket_keys()
}

/// Does a pull request with these texts name the ticket (compared ignoring case)?
fn names(branch: &str, title: &str, description: &str, key: &str) -> bool {
    named(branch, title, description)
        .iter()
        .any(|k| k.eq_ignore_ascii_case(key))
}

#[test]
fn finds_keys_in_branch_names_and_titles() {
    assert_eq!(ticket_keys("feature/BAU-142-fokus-ring"), ["BAU-142"]);
    assert_eq!(ticket_keys("agent/bau-142-fokus"), ["BAU-142"]);
    assert_eq!(ticket_keys("BAU-142: Fokus-Ring im Dark Mode"), ["BAU-142"]);
    assert_eq!(ticket_keys("[BAU-142] Fokus"), ["BAU-142"]);
    assert_eq!(ticket_keys("Fix für (bau-142)."), ["BAU-142"]);
    assert_eq!(ticket_keys("feature/bau_142_x"), Vec::<String>::new());
    assert_eq!(ticket_keys("fix_BAU-142_x"), ["BAU-142"], "_ trennt");
}

#[test]
fn case_does_not_matter() {
    assert_eq!(ticket_keys("Bau-7"), ["BAU-7"]);
    assert_eq!(ticket_keys("bAu-7"), ["BAU-7"]);
    assert!(names("", "", "see app-123", "APP-123"));
    assert!(names("feature/app-123-foo", "", "", "app-123"));
}

#[test]
fn key_must_sit_on_word_boundaries() {
    // No false hit: BAU-1420 is a different ticket than BAU-142.
    assert_eq!(ticket_keys("bau-1420-foo"), ["BAU-1420"]);
    assert!(!names("bau-1420-foo", "", "", "BAU-142"));
    assert!(!names("", "BAU-1420: Titel", "", "BAU-142"));
    assert!(!names("feature/app-1234-foo", "", "", "APP-123"));
    assert!(!names("APP-12", "", "", "APP-123"));
    // No letter or digit may stick to the front either.
    assert_eq!(ticket_keys("xbau-142"), ["XBAU-142"]);
    assert!(!names("xbau-142", "", "", "BAU-142"));
    assert!(!names("", "", "feature/xapp-123", "APP-123"));
    assert_eq!(ticket_keys("v2bau-142"), Vec::<String>::new());
    // And nothing alphanumeric at the back.
    assert!(!names("BAU-142a", "", "", "BAU-142"));
    assert_eq!(ticket_keys("BAU-142a"), Vec::<String>::new());
}

#[test]
fn no_false_positives_in_ordinary_text() {
    for text in [
        "",
        "-",
        "BAU",
        "BAU-",
        "-142",
        "BAU - 142",
        "BAU--142",
        "2026-11-11",
        "a-1",
        "Release v1-2",
        "feature/toggle-fokus",
    ] {
        assert_eq!(ticket_keys(text), Vec::<String>::new(), "{text:?}");
    }
}

#[test]
fn several_keys_keep_their_order_without_duplicates() {
    assert_eq!(
        ticket_keys("BAU-2 und bau-1, noch mal BAU-2"),
        ["BAU-2", "BAU-1"]
    );
    let body = "Fixes APP-123 and app-124, see also APP-1234.";
    assert!(names("", "", body, "APP-123"));
    assert!(names("", "", body, "APP-124"));
    assert!(!names("", "", body, "APP-12"));
}

#[test]
fn unicode_does_not_confuse_the_scanner() {
    assert_eq!(ticket_keys("Ärger-12 ü-3 BAU-5"), ["ÄRGER-12", "BAU-5"]);
}

#[test]
fn a_ticket_page_names_the_ticket_but_a_slug_does_not() {
    for text in [
        "https://acme.atlassian.net/browse/BAU-142",
        "see https://acme.atlassian.net/browse/BAU-142.",
        "[BAU-142](https://acme.atlassian.net/browse/BAU-142?focusedId=1)",
        "<https://acme.atlassian.net/browse/bau-142>",
        "https://acme.atlassian.net/jira/software/projects/BAU/boards/1?selectedIssue=BAU-142",
        "HTTPS://ACME.ATLASSIAN.NET/browse/BAU-142#comment",
    ] {
        assert_eq!(ticket_keys(text), ["BAU-142"], "{text:?}");
    }
    for text in [
        "https://example.com/blog/bau-142-story",
        "https://example.com/docs/rfc-0042.md",
        "https://example.com/page?ref=abc-1-x&id=app-9-b",
        "https://acme.atlassian.net/browse/BAU-142a",
        "https://acme.atlassian.net/browse/XBAU-142-x",
        "https://example.com/v2bau-142",
    ] {
        assert!(ticket_keys(text).is_empty(), "{text:?}");
    }
}

#[test]
fn text_around_an_address_still_counts() {
    assert_eq!(
        ticket_keys("BAU-1 https://example.com/blog/bau-2-story and APP-3"),
        ["BAU-1", "APP-3"]
    );
    assert_eq!(
        ticket_keys("(see https://example.com/x-1-y) APP-5"),
        ["APP-5"]
    );
}

#[test]
fn a_pull_request_names_tickets_in_branch_title_and_description() {
    assert_eq!(
        named("feature/bau-2-x", "BAU-1: Titel", ""),
        ["BAU-2", "BAU-1"]
    );
    assert_eq!(
        named("a", "b", "Part of BAU-142 and bau-150."),
        ["BAU-142", "BAU-150"]
    );
    // Each ticket once, in the order of the texts, Jira keys before GitHub issues.
    assert_eq!(
        named(
            "agent/app-1",
            "Fixes #12 (APP-2)",
            "APP-1, #12 and acme/web#3"
        ),
        ["APP-1", "APP-2", "acme/app#12", "acme/web#3"]
    );
    assert!(named("fix/typo", "Typo", "").is_empty());
}

#[test]
fn github_issues_are_named_by_number_by_repo_or_by_address() {
    assert!(names("", "", "Fixes #12", "acme/app#12"));
    assert!(names("", "Closes: #12", "", "acme/app#12"));
    assert!(names("", "", "Part of acme/web#3", "acme/web#3"));
    assert!(names(
        "",
        "",
        "Fixes https://github.com/acme/web/issues/9",
        "acme/web#9"
    ));
    assert!(names("", "", "github.com/acme/web/issues/9", "acme/web#9"));
    assert!(names(
        "",
        "",
        "https://www.github.com/acme/web/issues/9",
        "acme/web#9"
    ));
    // The repo and the case of the key do not have to match letter for letter.
    assert!(names("", "", "Fixes Acme/App#12", "acme/app#12"));
    // Without the repo of the pull request a short number names nothing.
    let unknown = PrTexts {
        repo: "",
        branch: "",
        title: "",
        description: "Fixes #12",
    };
    assert!(unknown.ticket_keys().is_empty());
}

#[test]
fn nothing_that_only_looks_like_an_issue_names_one() {
    for text in [
        "Fixes #120 and see #3",
        "relates to acme/web#12",
        "abc#12",
        "color #fff and #12ab",
        "fixes#12x",
        // Another host is not GitHub, another repo's issue is another issue.
        "https://example.com/acme/app/issues/12",
        "https://gitlab.com/acme/app/issues/12",
        "https://evilgithub.com/acme/app/issues/12",
        "https://github.com.evil.example/acme/app/issues/12",
        "https://github.com/acme/app/pull/12",
        "https://github.com/acme/app/pull/3/issues/12",
        "https://github.com/acme/other/issues/12",
        "https://example.com/page#12",
    ] {
        assert!(!names("", "", text, "acme/app#12"), "{text:?}");
    }
}

// ---------- One pull request, three uses ----------

/// What the three users of the rule say about one pull request (`acme/app#7`) and one ticket
/// of team `b` while team `a` is chosen: `[the ticket lists the pull request (`linked_reviews`),
/// the auto-link takes it, the team filter sees the link (and hides the pull request)]`.
/// `on_timeline`: the ticket's timeline lists the pull request.
fn three_uses(
    ticket_key: &str,
    branch: &str,
    title: &str,
    description: &str,
    on_timeline: bool,
) -> [bool; 3] {
    let mut w = crate::demo::workspace();
    let pr_template = w
        .items
        .iter()
        .find(|i| i.kind == Kind::PullRequest)
        .unwrap()
        .clone();
    let ticket_template = w
        .items
        .iter()
        .find(|i| i.kind == Kind::Ticket)
        .unwrap()
        .clone();
    let review_template = w.reviews[0].clone();
    w.items.clear();
    w.reviews.clear();

    let pr_key = PrKey::new("acme/app", 7);
    let mut ticket = ticket_template;
    ticket.id = "t1".into();
    ticket.key = ticket_key.into();
    ticket.scope = Scope::Team;
    ticket.team = Some(TeamRef::new("b", "Team b"));
    ticket.pr = None;
    ticket.branch = None;
    ticket.project = on_timeline.then(|| ProjectItem {
        linked_prs: vec![pr_key.clone()],
        ..Default::default()
    });
    let mut pr = pr_template;
    pr.id = "pr7".into();
    pr.scope = Scope::Team;
    pr.branch = Some(branch.into());
    pr.title = title.into();
    pr.description = description.into();
    let entry = pr.pr.as_mut().unwrap();
    entry.number = 7;
    entry.repo = "acme/app".into();
    let mut review = review_template;
    review.number = 7;
    review.repo = "acme/app".into();
    review.branch = branch.into();
    review.title = title.into();
    review.description = description.into();
    review.ticket_key = None;
    review.merged = None;
    w.items = vec![ticket, pr];
    w.reviews = vec![review];
    w.team_choice = TeamChoice {
        teams: vec![TeamRef::new("a", "Team a")],
        include_no_team: false,
    };

    [
        !w.linked_reviews("t1").is_empty(),
        w.auto_links() == [("t1".to_string(), pr_key)],
        !w.team_visible(&w.items[1]),
    ]
}

#[test]
fn the_same_pull_request_belongs_to_the_same_tickets_for_list_auto_link_and_team_filter() {
    for (key, branch, title, description) in [
        // A Jira key in each of the three texts, in either case.
        ("APP-1", "feature/app-1-focus", "Focus", ""),
        ("APP-1", "mira/focus", "APP-1: Focus", ""),
        ("APP-1", "mira/focus", "Focus", "Part of app-1."),
        (
            "APP-1",
            "mira/focus",
            "Focus",
            "Ticket: https://acme.atlassian.net/browse/APP-1",
        ),
        // A GitHub issue in each of the three texts.
        ("acme/app#12", "mira/focus", "Focus (#12)", ""),
        ("acme/app#12", "mira/focus", "Focus", "Fixes #12"),
        ("acme/app#12", "mira/focus", "Focus", "Closes acme/app#12"),
        (
            "acme/app#12",
            "mira/focus",
            "Focus",
            "Fixes https://github.com/acme/app/issues/12",
        ),
    ] {
        assert_eq!(
            three_uses(key, branch, title, description, false),
            [true; 3],
            "{key}: {branch} | {title} | {description}"
        );
    }
}

#[test]
fn a_pull_request_the_timeline_lists_belongs_to_the_ticket_for_all_three() {
    assert_eq!(
        three_uses("acme/app#12", "mira/focus", "Focus", "", true),
        [true; 3]
    );
}

#[test]
fn a_pull_request_that_does_not_name_the_ticket_belongs_to_none() {
    for (key, branch, title, description) in [
        ("APP-1", "mira/focus", "Focus", ""),
        (
            "APP-1",
            "feature/app-12-focus",
            "APP-10",
            "APP-100, XAPP-1, APP-1a",
        ),
        (
            "APP-1",
            "mira/focus",
            "Focus",
            "https://example.com/blog/app-1-story",
        ),
        ("acme/app#12", "mira/focus", "Focus", "Fixes #120, see #1"),
        ("acme/app#12", "mira/focus", "Focus", "Fixes acme/web#12"),
        (
            "acme/app#12",
            "mira/focus",
            "Focus",
            "Fixes https://example.com/acme/app/issues/12",
        ),
    ] {
        assert_eq!(
            three_uses(key, branch, title, description, false),
            [false; 3],
            "{key}: {branch} | {title} | {description}"
        );
    }
}
