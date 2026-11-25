use super::*;
use crate::agent::Closure;
use cockpit_i18n::Lang;

fn facts() -> DeleteFacts<'static> {
    DeleteFacts {
        pattern: DEFAULT_PATTERN,
        base_branch: "main",
        default_branch: "main",
        branch: "claude/BAU-1-x",
        pull_head: "claude/BAU-1-x",
        run_branch: "claude/BAU-1-x",
        protected: false,
        ended: Some(Closure::Merged),
    }
}

#[test]
fn a_merged_or_closed_agent_branch_may_go() {
    assert!(deletable(&facts()).is_ok());
    let closed = DeleteFacts {
        ended: Some(Closure::Closed),
        ..facts()
    };
    assert!(deletable(&closed).is_ok());
}

#[test]
fn an_open_pull_request_keeps_its_branch() {
    let e = deletable(&DeleteFacts {
        ended: None,
        ..facts()
    })
    .unwrap_err();
    assert!(e.render(Lang::De).contains("noch offen"), "{e:?}");
}

#[test]
fn base_default_and_protected_branches_stay() {
    for branch in ["main", "master", "develop", "release", "refs/heads/x"] {
        let f = DeleteFacts {
            branch,
            pull_head: branch,
            run_branch: branch,
            ..facts()
        };
        assert!(deletable(&f).is_err(), "{branch}");
    }
    // The base branch of the repo, even with a pattern that would match it.
    let f = DeleteFacts {
        pattern: "{ticket}-{slug}",
        base_branch: "BAU-1-x",
        ..facts()
    };
    assert!(deletable(&f).is_err());
    let f = DeleteFacts {
        default_branch: "claude/BAU-1-x",
        ..facts()
    };
    assert!(
        deletable(&f)
            .unwrap_err()
            .render(Lang::De)
            .contains("Standard-Branch")
    );
    let f = DeleteFacts {
        protected: true,
        ..facts()
    };
    assert!(
        deletable(&f)
            .unwrap_err()
            .render(Lang::De)
            .contains("geschützt")
    );
}

#[test]
fn a_name_outside_the_pattern_stays() {
    let f = DeleteFacts {
        branch: "feature/BAU-1-x",
        pull_head: "feature/BAU-1-x",
        run_branch: "feature/BAU-1-x",
        ..facts()
    };
    assert!(
        deletable(&f)
            .unwrap_err()
            .render(Lang::De)
            .contains("Muster")
    );
}

#[test]
fn only_the_branch_of_the_run_and_its_pull_request() {
    let f = DeleteFacts {
        pull_head: "claude/BAU-2-y",
        ..facts()
    };
    assert!(
        deletable(&f)
            .unwrap_err()
            .render(Lang::De)
            .contains("gehört nicht")
    );
    let f = DeleteFacts {
        run_branch: "claude/BAU-2-y",
        ..facts()
    };
    assert!(deletable(&f).is_err());
}

#[test]
fn odd_names_are_refused() {
    for branch in ["", "-x", "claude/../x", "claude/a b", "claude/x?y"] {
        let f = DeleteFacts {
            branch,
            pull_head: branch,
            run_branch: branch,
            ..facts()
        };
        assert!(deletable(&f).is_err(), "{branch:?}");
    }
}
