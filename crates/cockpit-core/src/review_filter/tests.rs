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
    issue.linked_prs = vec![PrKey::new(crate::demo::DEMO_REPO, 101)];
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
    assert!(!repos.iter().any(|r| r == crate::demo::DEMO_REPO));
}

#[test]
fn the_open_review_stays_even_if_the_filter_hides_it() {
    let ws = demo::seed();
    let hidden = ws
        .reviews
        .iter()
        .find(|r| r.repo == crate::demo::DEMO_REPO)
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
    assert!(all.contains(&crate::demo::DEMO_REPO));
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

// ---------- Sections ----------

fn section(repo: &str, keys: &[u32], attention: usize) -> ReviewSection {
    ReviewSection {
        repo: repo.into(),
        keys: keys.iter().map(|n| PrKey::new(repo, *n)).collect(),
        attention,
    }
}

/// The sample data where `acme/shop#103` and `acme/design-tokens#102` ask for my review.
fn asking() -> Workspace {
    let mut ws = demo::seed();
    for item in &mut ws.items {
        if let Some(pr) = item.pr.as_mut()
            && ((pr.repo == demo::DEMO_REPO_SHOP && pr.number == 103)
                || (pr.repo == demo::DEMO_REPO_TOKENS && pr.number == 102))
        {
            pr.requested_me = true;
        }
    }
    ws
}

#[test]
fn sections_are_per_repo_sorted_by_attention_then_name_and_keep_the_order_inside() {
    let ws = asking();
    let list = ws.reviews_filtered(&ReviewFilter::default(), None);
    let sections = ws.review_sections(&list);
    let names: Vec<_> = sections.iter().map(|s| s.repo.as_str()).collect();
    // Review requested from me first (design-tokens and shop), then the rest by name.
    assert_eq!(
        names,
        [
            demo::DEMO_REPO_TOKENS,
            demo::DEMO_REPO_SHOP,
            crate::demo::DEMO_REPO
        ]
    );
    assert_eq!(sections[0].attention, 1);
    assert_eq!(sections[2].attention, 0);
    let in_app: Vec<u32> = list
        .iter()
        .filter(|r| r.repo == crate::demo::DEMO_REPO)
        .map(|r| r.number)
        .collect();
    let section_numbers: Vec<u32> = sections[2].keys.iter().map(|k| k.number).collect();
    assert_eq!(
        section_numbers, in_app,
        "the order inside a section is the list's"
    );
    assert_eq!(
        sections.iter().map(|s| s.keys.len()).sum::<usize>(),
        list.len()
    );
}

#[test]
fn a_hidden_repo_has_no_section() {
    let ws = asking();
    let f = ReviewFilter {
        repos: vec![demo::DEMO_REPO_SHOP.into()],
        ..Default::default()
    };
    let list = ws.reviews_filtered(&f, None);
    let sections = ws.review_sections(&list);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].repo, demo::DEMO_REPO_SHOP);
}

#[test]
fn my_own_pull_request_with_requested_changes_needs_attention() {
    let mut ws = demo::seed();
    let review = ws.reviews[0].clone();
    assert!(!ws.review_needs_attention(&review));
    let key = review.key();
    let pr = ws
        .items
        .iter_mut()
        .find_map(|i| i.pr.as_mut().filter(|p| p.key().same(&key)))
        .unwrap();
    pr.mine = true;
    pr.changes_requested = true;
    assert!(ws.review_needs_attention(&review));
    ws.items
        .iter_mut()
        .find_map(|i| i.pr.as_mut().filter(|p| p.key().same(&key)))
        .unwrap()
        .merged = true;
    assert!(
        !ws.review_needs_attention(&review),
        "a merged one needs nothing"
    );
}

#[test]
fn owners_group_first_only_with_many_repos_and_more_than_one_owner() {
    let many: Vec<ReviewSection> = (0..4)
        .map(|i| section(&format!("a/r{i}"), &[1], 0))
        .chain((0..4).map(|i| section(&format!("b/r{i}"), &[1], usize::from(i == 3))))
        .collect();
    let groups = group_sections(many.clone(), OWNER_GROUPS_FROM);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].owner.as_deref(), Some("b"), "attention first");
    assert_eq!(groups[0].count(), 4);
    assert_eq!(groups[1].owner.as_deref(), Some("a"));
    // Few repos, or one owner only: a single group without an owner header.
    let few = group_sections(many[..3].to_vec(), OWNER_GROUPS_FROM);
    assert_eq!(few.len(), 1);
    assert_eq!(few[0].owner, None);
    let one_owner: Vec<_> = (0..9)
        .map(|i| section(&format!("a/r{i}"), &[1], 0))
        .collect();
    assert_eq!(group_sections(one_owner, OWNER_GROUPS_FROM)[0].owner, None);
}

#[test]
fn collapsed_sections_drop_out_of_the_visible_order_except_the_open_one() {
    let groups = group_sections(
        vec![
            section("a/x", &[1, 2], 0),
            section("b/y", &[3], 0),
            section("b/z", &[4], 0),
        ],
        2,
    );
    let nums = |keys: Vec<PrKey>| keys.iter().map(|k| k.number).collect::<Vec<_>>();
    assert_eq!(nums(visible_keys(&groups, &[], None)), [1, 2, 3, 4]);
    // A collapsed repo is skipped; a collapsed owner takes all its repos along.
    assert_eq!(nums(visible_keys(&groups, &["A/X".into()], None)), [3, 4]);
    assert_eq!(nums(visible_keys(&groups, &["b".into()], None)), [1, 2]);
    // The open pull request keeps its section expanded (deep link).
    let open = PrKey::new("b/y", 3);
    assert_eq!(
        nums(visible_keys(&groups, &["b".into()], Some(&open))),
        [1, 2, 3]
    );
    assert!(is_collapsed(&["b".into()], Some("b"), "b/y"));
    assert!(!is_collapsed(&["b/z".into()], Some("b"), "b/y"));
}

#[test]
fn up_and_down_walk_the_visible_list_and_stop_at_the_ends() {
    let keys: Vec<PrKey> = [1, 2, 3].iter().map(|n| PrKey::new("a/x", *n)).collect();
    let at = |n: u32| PrKey::new("a/x", n);
    assert_eq!(step_key(&keys, Some(&at(1)), true), Some(at(2)));
    assert_eq!(step_key(&keys, Some(&at(3)), true), Some(at(3)));
    assert_eq!(step_key(&keys, Some(&at(1)), false), Some(at(1)));
    assert_eq!(step_key(&keys, Some(&at(3)), false), Some(at(2)));
    assert_eq!(step_key(&keys, None, true), Some(at(1)));
    assert_eq!(step_key(&[], None, true), None);
}
