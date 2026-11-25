//! How an outward call is sent: the route of every variant and the choice among transitions.

use super::*;

#[test]
fn the_first_transition_to_a_status_wins() {
    // Two transitions lead to "Done": the order Jira lists them in decides, not their names.
    let list: Vec<Transition> = serde_json::from_value(json!([
        {"id": "41", "name": "Zebra", "to": {"name": "Done", "statusCategory": {"key": "done"}}},
        {"id": "42", "name": "Done", "to": {"name": "Done", "statusCategory": {"key": "done"}}}
    ]))
    .unwrap();
    assert_eq!(
        choose_transition(&list, TicketStatus::Done).unwrap().id,
        "41"
    );
}

#[test]
fn every_call_goes_through_exactly_one_route() {
    let issue = |write| Outbound::Issue {
        repo: "a/x".into(),
        number: 1,
        write,
    };
    let calls = [
        (
            Outbound::JiraComment {
                key: "ABC-1".into(),
                text: "t".into(),
            },
            Route::Jira,
        ),
        (
            Outbound::JiraStatus {
                key: "ABC-1".into(),
                status: TicketStatus::Done,
            },
            Route::Jira,
        ),
        (
            Outbound::GitHubComment {
                repo: "a/x".into(),
                number: 1,
                text: "t".into(),
            },
            Route::GitHub,
        ),
        (
            Outbound::GitHubBranch {
                repo: "a/x".into(),
                name: "b".into(),
            },
            Route::GitHub,
        ),
        (
            Outbound::GitHubReview {
                repo: "a/x".into(),
                number: 1,
                review: review_data(&review(), "", ""),
            },
            Route::GitHub,
        ),
        (
            Outbound::GitHubMerge {
                repo: "a/x".into(),
                number: 1,
                method: MergeMethod::Squash,
                sha: String::new(),
            },
            Route::GitHub,
        ),
        (
            Outbound::GitHubRecheck {
                repo: "a/x".into(),
                number: 1,
                check: "build".into(),
            },
            Route::GitHub,
        ),
        (
            Outbound::ProjectField {
                project_id: "P".into(),
                item_id: "I".into(),
                field_id: "F".into(),
                value: None,
            },
            Route::Projects,
        ),
        (
            Outbound::DraftEdit {
                draft_id: "D".into(),
                title: "t".into(),
                body: "b".into(),
            },
            Route::Projects,
        ),
        (
            issue(crate::github::projects::IssueWrite::Comment("t".into())),
            Route::Projects,
        ),
    ];
    for (call, route) in &calls {
        assert_eq!(call.route(), *route, "{call:?}");
    }
    // The name for messages follows the route.
    let services: Vec<&str> = calls.iter().map(|(c, _)| c.service()).collect();
    assert_eq!(
        services,
        [
            "Jira", "Jira", "GitHub", "GitHub", "GitHub", "GitHub", "GitHub", "GitHub", "GitHub",
            "GitHub"
        ]
    );
}
