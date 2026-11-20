use super::*;
use crate::demo;

/// The sample data plus a colleague's pull request (`acme/app#101`) on the timeline of the
/// sample project's issue.
fn linked() -> Workspace {
    let mut ws = demo::seed();
    let issue = ws
        .items
        .iter_mut()
        .find(|i| i.id == "gh-acme-app-31")
        .and_then(|i| i.project.as_mut())
        .unwrap();
    issue.linked_prs = vec![PrKey::new(crate::DEMO_REPO, 101)];
    ws
}

fn repos_of(list: &[&Review]) -> Vec<String> {
    list.iter().map(|r| r.repo.clone()).collect()
}

#[test]
fn the_default_shows_everything_of_everyone() {
    let ws = demo::seed();
    let f = ReviewFilter::default();
    assert!(f.is_default() && !f.narrows());
    assert_eq!(ws.reviews_filtered(&f, None).len(), ws.reviews.len());
}

#[test]
fn one_repo_keeps_only_its_pull_requests() {
    let ws = demo::seed();
    let f = ReviewFilter {
        repos: vec!["ACME/Shop".into()],
        ..Default::default()
    };
    let list = ws.reviews_filtered(&f, None);
    assert!(!list.is_empty());
    assert!(list.iter().all(|r| r.repo == demo::DEMO_REPO_SHOP));
}

#[test]
fn several_repos_are_an_or() {
    let ws = demo::seed();
    let f = ReviewFilter {
        repos: vec![demo::DEMO_REPO_SHOP.into(), demo::DEMO_REPO_TOKENS.into()],
        ..Default::default()
    };
    let repos = repos_of(&ws.reviews_filtered(&f, None));
    assert!(repos.iter().any(|r| r == demo::DEMO_REPO_SHOP));
    assert!(repos.iter().any(|r| r == demo::DEMO_REPO_TOKENS));
    assert!(!repos.iter().any(|r| r == crate::DEMO_REPO));
}

#[test]
fn the_open_review_stays_even_if_the_filter_hides_it() {
    let ws = demo::seed();
    let hidden = ws
        .reviews
        .iter()
        .find(|r| r.repo == crate::DEMO_REPO)
        .unwrap()
        .key();
    let f = ReviewFilter {
        repos: vec![demo::DEMO_REPO_SHOP.into()],
        ..Default::default()
    };
    let list = ws.reviews_filtered(&f, Some(&hidden));
    assert!(list.iter().any(|r| r.key().same(&hidden)));
}

#[test]
fn ownership_and_repo_combine() {
    let ws = demo::seed();
    let f = ReviewFilter {
        ownership: Ownership::Mine,
        repos: vec![demo::DEMO_REPO_SHOP.into()],
        ..Default::default()
    };
    // The colleague's pull request in the shop repo does not concern me.
    assert!(ws.reviews_filtered(&f, None).is_empty());
}

#[test]
fn the_project_filter_keeps_pull_requests_linked_to_its_items() {
    let ws = linked();
    let f = ReviewFilter {
        projects: vec!["PVT_demo".into()],
        ..Default::default()
    };
    let list = ws.reviews_filtered(&f, None);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].number, 101);
    let other = ReviewFilter {
        projects: vec!["PVT_other".into()],
        ..Default::default()
    };
    assert!(ws.reviews_filtered(&other, None).is_empty());
}

#[test]
fn repo_and_project_narrow_together() {
    let ws = linked();
    let f = ReviewFilter {
        repos: vec![demo::DEMO_REPO_SHOP.into()],
        projects: vec!["PVT_demo".into()],
        ..Default::default()
    };
    assert!(ws.reviews_filtered(&f, None).is_empty());
}

#[test]
fn groups_are_by_owner_with_counts_and_a_search() {
    let g = group_repos(["b/x", "a/z", "a/y", "a/y", "B/w", ""].into_iter(), "");
    assert_eq!(g.len(), 2);
    assert_eq!(g[0].owner, "a");
    assert_eq!(g[0].repos, [("a/y".to_string(), 2), ("a/z".to_string(), 1)]);
    assert_eq!(g[0].count(), 3);
    // Owners are compared without regard to case.
    assert_eq!(g[1].repos.len(), 2);
    let found = group_repos(["a/y", "a/z", "b/x"].into_iter(), "b x");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].repos, [("b/x".to_string(), 1)]);
    assert!(group_repos(std::iter::empty(), "").is_empty());
}

#[test]
fn the_choices_come_from_the_reviews() {
    let ws = linked();
    let groups = ws.review_repo_groups(Ownership::All, "");
    let all: Vec<&str> = groups
        .iter()
        .flat_map(|g| g.repos.iter().map(|(r, _)| r.as_str()))
        .collect();
    assert!(all.contains(&demo::DEMO_REPO_SHOP));
    assert!(all.contains(&crate::DEMO_REPO));
    assert!(all.contains(&demo::DEMO_REPO_TOKENS));
    let projects = ws.review_projects();
    assert_eq!(projects.len(), 1);
    assert_eq!(
        (projects[0].id.as_str(), projects[0].count),
        ("PVT_demo", 1)
    );
}

#[test]
fn toggling_adds_and_removes() {
    let mut f = ReviewFilter::default();
    f.toggle_repo("a/b");
    f.toggle_repo("c/d");
    f.toggle_repo("A/B");
    assert_eq!(f.repos, ["c/d"]);
    f.toggle_project("PVT_1");
    assert!(f.narrows());
    f.toggle_project("PVT_1");
    f.toggle_repo("c/d");
    assert!(f.is_default());
}

#[test]
fn an_old_workspace_without_the_filter_loads_with_the_default() {
    let json = serde_json::to_value(demo::workspace()).unwrap();
    let mut obj = json.as_object().unwrap().clone();
    obj.remove("review_filter");
    let ws: Workspace = serde_json::from_value(obj.into()).unwrap();
    assert!(ws.review_filter.is_default());
    let f: ReviewFilter = serde_json::from_str("{}").unwrap();
    assert!(f.is_default());
}
