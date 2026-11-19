//! The suggestions for the start dialog come from what the workspace already knows.

use super::*;
use cockpit_core::repos::suggest::SuggestReason;

#[test]
fn earlier_runs_of_the_project_are_remembered_without_any_configuration() {
    let w = welt(true);
    let state = with_repos(&w, vec![], None);
    let mut ws = state.inner.live.copy();
    let mut run = ws.runs[0].clone();
    run.ticket_key = "BAU-142".into();
    run.repo = "firma/web".into();
    ws.runs.push(run);
    state.inner.live.set_state_for_test(ws);

    let s = state.repo_suggestions("BAU-142", None).suggestions;
    assert_eq!(s[0].repo, "firma/web");
    assert!(matches!(
        &s[0].reason,
        SuggestReason::PreviousRun { project, runs } if project == "BAU" && *runs >= 1
    ));
}

#[test]
fn pull_requests_of_the_ticket_and_the_configured_rules_are_suggested_too() {
    let w = welt(true);
    let rules = vec![cockpit_core::AgentRepo {
        projects: vec!["BAU".into()],
        ..cockpit_core::AgentRepo::new("firma/rule")
    }];
    let state = with_repos(&w, rules, None);
    let mut ws = state.inner.live.copy();
    ws.runs.clear();
    // A review that is linked to the ticket lives in firma/linked.
    let mut review = ws.reviews[0].clone();
    review.repo = "firma/linked".into();
    review.ticket_key = Some("BAU-142".into());
    ws.reviews.push(review);
    state.inner.live.set_state_for_test(ws);

    let s = state.repo_suggestions("BAU-142", None).suggestions;
    let list: Vec<_> = s.iter().map(|s| (s.repo.as_str(), &s.reason)).collect();
    assert_eq!(list[0], ("firma/linked", &SuggestReason::LinkedPr));
    assert!(list.contains(&(
        "firma/rule",
        &SuggestReason::Rule {
            project: "BAU".into()
        }
    )));
}

#[test]
fn repos_named_in_the_ticket_text_count_when_github_shows_them() {
    let w = welt(true);
    let state = with_repos(&w, vec![], None);
    let mut ws = state.inner.live.copy();
    ws.runs.clear();
    ws.items
        .iter_mut()
        .find(|i| i.key == "BAU-142")
        .unwrap()
        .description = "Das passiert in firma/shop, siehe docs/readme.".into();
    state.inner.live.set_state_for_test(ws);

    let visible = vec!["firma/shop".to_string()];
    let s = state
        .repo_suggestions("BAU-142", Some(&visible))
        .suggestions;
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].repo, "firma/shop");
    assert_eq!(s[0].reason, SuggestReason::Mentioned);
    // Without a list of visible repos a bare name is not trusted.
    assert!(
        state
            .repo_suggestions("BAU-142", None)
            .suggestions
            .is_empty()
    );
}
