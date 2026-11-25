//! The check before clone and push against the configured pattern of the repo.

use super::*;
use cockpit_i18n::Lang;

fn with<'a>(branch: &'a str, pattern: &'a str) -> BranchRule<'a> {
    BranchRule {
        branch,
        pattern,
        base: "main",
        existing: false,
    }
}

const FEATURE: &str = "feature/{ticket}-{slug}";

#[test]
fn a_branch_of_a_custom_pattern_is_cloned_and_pushed() {
    let a = layout();
    let r = with("feature/BAU-1-fokus", FEATURE);
    let base = clone(&a.url, &a.clone, None, &r).unwrap();
    agent_works(&a.clone, "src/neu.rs");
    publish(
        &a.clone,
        &a.export,
        &base,
        &a.url,
        None,
        &RepoRules::default(),
        &r,
    )
    .unwrap();
    let mut h = heads(&a.remote);
    h.sort();
    assert_eq!(h, ["feature/BAU-1-fokus", "main"]);
}

#[test]
fn a_branch_outside_the_pattern_is_never_cloned_or_pushed() {
    let a = layout();
    // The default name does not fit the repo's pattern.
    let e = clone(&a.url, &a.clone, None, &with("claude/BAU-1-x", FEATURE))
        .unwrap_err()
        .render(Lang::De);
    assert!(e.contains("passt nicht zum Muster"), "{e}");
    assert!(!a.clone.exists());

    // The container reports a name that is not the planned one: publish refuses it too.
    let r = with("feature/BAU-1-x", FEATURE);
    let base = clone(&a.url, &a.clone, None, &r).unwrap();
    agent_works(&a.clone, "a.txt");
    let wrong = with("claude/BAU-1-x", FEATURE);
    let e = publish(
        &a.clone,
        &a.export,
        &base,
        &a.url,
        None,
        &RepoRules::default(),
        &wrong,
    )
    .unwrap_err()
    .render(Lang::De);
    assert!(e.contains("passt nicht zum Muster"), "{e}");
    assert_eq!(heads(&a.remote), ["main"]);
}

#[test]
fn protected_names_never_get_through_whatever_the_pattern_is() {
    let a = layout();
    for name in [
        "main",
        "master",
        "develop",
        "HEAD",
        "trunk",
        "refs/heads/main",
    ] {
        for pattern in ["{ticket}", FEATURE, "{ticket}-{slug}"] {
            let r = with(name, pattern);
            assert!(
                clone(&a.url, &a.clone, None, &r).is_err(),
                "{name} {pattern}"
            );
            assert!(
                publish(
                    &a.clone,
                    &a.export,
                    "abcdef1",
                    &a.url,
                    None,
                    &RepoRules::default(),
                    &r
                )
                .is_err(),
                "{name} {pattern}"
            );
        }
    }
    // The base branch of the repo counts as protected, even under an odd name.
    let r = BranchRule {
        base: "release-1",
        ..with("release-1", "{ticket}")
    };
    assert!(r.check().is_err());
    assert!(!a.clone.exists());
    assert_eq!(heads(&a.remote), ["main"]);
}

#[test]
fn an_existing_foreign_branch_is_never_used() {
    let a = layout();
    // Somebody else's branch with a name that fits the pattern.
    g(&a.remote, &["branch", "feature/BAU-7-fremd", "main"]);
    let foreign = g(&a.remote, &["rev-parse", "feature/BAU-7-fremd"]);
    let r = with("feature/BAU-7-fremd", FEATURE);
    let e = clone(&a.url, &a.clone, None, &r)
        .unwrap_err()
        .render(Lang::De);
    assert!(e.contains("gibt es im Repo schon"), "{e}");
    assert!(!a.clone.exists());

    // And when it appears while the agent works, the push is refused as well.
    let r2 = with("feature/BAU-8-x", FEATURE);
    let base = clone(&a.url, &a.clone, None, &r2).unwrap();
    agent_works(&a.clone, "a.txt");
    g(&a.remote, &["branch", "feature/BAU-8-x", "main"]);
    let e = publish(
        &a.clone,
        &a.export,
        &base,
        &a.url,
        None,
        &RepoRules::default(),
        &r2,
    )
    .unwrap_err()
    .render(Lang::De);
    assert!(e.contains("gibt es im Repo schon"), "{e}");
    assert_eq!(g(&a.remote, &["rev-parse", "feature/BAU-7-fremd"]), foreign);
}

#[test]
fn a_followup_may_keep_a_branch_of_the_default_pattern() {
    let a = layout();
    // The pull request was opened under the default pattern, before the repo's pattern changed.
    let first = clone(&a.url, &a.clone, None, &rule("claude/BAU-1-x")).unwrap();
    agent_works(&a.clone, "alt.txt");
    publish(
        &a.clone,
        &a.export,
        &first,
        &a.url,
        None,
        &RepoRules::default(),
        &rule("claude/BAU-1-x"),
    )
    .unwrap();
    let again = a.remote.parent().unwrap().join("klon2");
    let r = BranchRule {
        existing: true,
        ..with("claude/BAU-1-x", FEATURE)
    };
    assert!(clone_branch(&a.url, &again, None, &r).is_ok());
    // But never a protected or unrelated branch.
    for bad in ["main", "somebody/else"] {
        let r = BranchRule {
            existing: true,
            ..with(bad, FEATURE)
        };
        assert!(
            clone_branch(&a.url, &a.remote.parent().unwrap().join("k3"), None, &r).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn a_new_run_cannot_pose_as_a_followup_and_the_other_way_round() {
    let a = layout();
    let r = BranchRule {
        existing: true,
        ..rule("claude/BAU-1-x")
    };
    assert!(clone(&a.url, &a.clone, None, &r).is_err());
    assert!(clone_branch(&a.url, &a.clone, None, &rule("claude/BAU-1-x")).is_err());
}

#[test]
fn guidelines_are_found_in_the_clone_in_the_usual_places() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(guideline_file(t.path()), None);
    std::fs::create_dir_all(t.path().join("docs")).unwrap();
    std::fs::write(t.path().join("docs/CONTRIBUTING.md"), "x").unwrap();
    assert_eq!(guideline_file(t.path()), Some("docs/CONTRIBUTING.md"));
    std::fs::write(t.path().join("CONTRIBUTING.md"), "x").unwrap();
    assert_eq!(guideline_file(t.path()), Some("CONTRIBUTING.md"));
}

#[cfg(unix)]
#[test]
fn a_linked_guideline_file_is_ignored() {
    let t = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/etc/passwd", t.path().join("CONTRIBUTING.md")).unwrap();
    assert_eq!(guideline_file(t.path()), None);
}
