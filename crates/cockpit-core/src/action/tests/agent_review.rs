use super::*;
use crate::review::{AgentComment, DiffSide};

/// The key of the review run of pull request #91 in the sample repo.
fn key() -> String {
    format!("prreview-{}", PrKey::new(crate::demo::DEMO_REPO, 91).slug())
}

/// `(file, line number of an added line, line number of a removed line)` of the first review with both.
fn spots(w: &Workspace, n: u32) -> (String, u32, u32) {
    let f = &w.review(n).unwrap().files[0];
    let add = f
        .lines
        .iter()
        .find(|l| l.kind == crate::LineKind::Add)
        .unwrap();
    let del = f
        .lines
        .iter()
        .find(|l| l.kind == crate::LineKind::Del)
        .map_or(0, |l| l.no);
    (f.name.clone(), add.no, del)
}

fn comment(path: &str, line: u32, text: &str) -> AgentReport {
    AgentReport::ReviewComment {
        path: path.into(),
        line,
        start_line: None,
        side: DiffSide::Right,
        text: text.into(),
        suggestion: None,
    }
}

fn started() -> Workspace {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartPrReview {
            review: 91.into(),
            language: None,
            repo_language: None,
        },
        1,
    );
    report(
        &mut w,
        &key(),
        AgentReport::Started {
            detail: cockpit_i18n::Msg::raw("docker"),
        },
    )
    .unwrap();
    w
}

#[test]
fn only_the_user_starts_a_review_run_and_it_is_a_read_only_job() {
    let mut w = ws();
    assert!(
        w.apply(
            Action::StartPrReview {
                review: 91.into(),
                language: None,
                repo_language: None
            },
            Actor::Agent,
            T
        )
        .is_err()
    );
    ok(
        &mut w,
        Action::StartPrReview {
            review: 91.into(),
            language: None,
            repo_language: None,
        },
        1,
    );
    let run = w.runs.iter().find(|r| r.key() == key()).unwrap();
    assert!(run.is_running());
    assert!(!run.options.plan_first, "your click is the approval");
    // Works on any branch, not only `claude/…`.
    assert!(!run.branch().is_empty());
    fails(
        &mut w,
        Action::StartPrReview {
            review: 91.into(),
            language: None,
            repo_language: None,
        },
        ActionError::Agent(AgentError::AlreadyRunning),
    );
}

#[test]
fn drafts_are_checked_against_the_diff_and_wait_for_a_decision() {
    let mut w = started();
    let (file, add, _) = spots(&w, 91);
    report(&mut w, &key(), comment(&file, add, "Fokus fehlt")).unwrap();
    // Unknown file, line that does not exist, removed line on the right side: dropped.
    assert!(report(&mut w, &key(), comment("gibt/es/nicht.rs", add, "x")).is_err());
    assert!(report(&mut w, &key(), comment(&file, 99_999, "x")).is_err());
    report(
        &mut w,
        &key(),
        AgentReport::ReviewSummary {
            text: "Insgesamt gut.".into(),
        },
    )
    .unwrap();
    report(
        &mut w,
        &key(),
        AgentReport::Done {
            summary: cockpit_i18n::Msg::raw("1 Kommentar als Entwurf vorbereitet."),
        },
    )
    .unwrap();
    let r = w.review(91).unwrap();
    assert!(r.agent_reviewed);
    assert_eq!(r.agent_summary, "Insgesamt gut.");
    let d = r.suggestions.first().unwrap();
    assert_eq!(
        (d.path.as_str(), d.line, d.decision),
        (file.as_str(), add, None)
    );
    // Nothing goes out while a draft is undecided.
    assert!(!r.can_send());
    // After the run nothing more is accepted.
    assert!(report(&mut w, &key(), comment(&file, add, "noch einer")).is_err());
}

#[test]
fn a_failed_review_run_leaves_the_reason_and_the_old_state_is_replaced_on_restart() {
    let mut w = started();
    let (file, add, _) = spots(&w, 91);
    report(&mut w, &key(), comment(&file, add, "Entwurf")).unwrap();
    report(
        &mut w,
        &key(),
        AgentReport::Failed {
            reason: cockpit_i18n::Msg::raw("Zeitlimit"),
        },
    )
    .unwrap();
    // The run stays (idle) with its reason, so the page can show it.
    let run = w.runs.iter().find(|r| r.key() == key()).unwrap();
    assert!(run.is_idle());
    assert_eq!(run.ended, Some(cockpit_i18n::Msg::raw("Zeitlimit")));
    assert!(!w.review(91).unwrap().agent_reviewed);
    // A new run replaces the old drafts.
    ok(
        &mut w,
        Action::StartPrReview {
            review: 91.into(),
            language: None,
            repo_language: None,
        },
        1,
    );
    assert_eq!(w.review(91).unwrap().agent_drafts(), 0);
    assert!(
        w.runs
            .iter()
            .find(|r| r.key() == key())
            .unwrap()
            .ended
            .is_none()
    );
}

#[test]
fn reports_of_other_runs_cannot_write_drafts() {
    let mut w = ws();
    ok(
        &mut w,
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: direct(),
        },
        1,
    );
    let (file, add, _) = spots(&w, 91);
    assert!(report(&mut w, "BAU-142", comment(&file, add, "x")).is_err());
    assert_eq!(w.review(91).unwrap().agent_drafts(), 0);
}

#[test]
fn approved_drafts_are_the_only_ones_that_go_out_with_code_and_verdict_comment() {
    let mut w = started();
    let (file, add, _) = spots(&w, 91);
    for t in ["Eins", "Zwei"] {
        report(
            &mut w,
            &key(),
            AgentReport::ReviewComment {
                path: file.clone(),
                line: add,
                start_line: None,
                side: DiffSide::Right,
                text: t.into(),
                suggestion: (t == "Eins").then(|| "neu();".to_string()),
            },
        )
        .unwrap();
    }
    report(
        &mut w,
        &key(),
        AgentReport::Done {
            summary: cockpit_i18n::Msg::raw("ok"),
        },
    )
    .unwrap();
    let r = w.review(91).unwrap();
    let ids: Vec<String> = r.suggestions.iter().map(|s| s.id.clone()).collect();
    let (first, second) = (ids[0].clone(), ids[1].clone());
    assert_eq!(r.verdict, crate::Verdict::Comment);
    ok(
        &mut w,
        Action::Decide {
            review: 91.into(),
            suggestion: first,
            decision: Some(crate::Decision::Approved),
        },
        1,
    );
    let open = w.review(91).unwrap().undecided();
    fails(
        &mut w,
        Action::SendReview {
            review: 91.into(),
            comment: String::new(),
        },
        ActionError::Send(crate::SendError::Undecided(open)),
    );
    ok(
        &mut w,
        Action::Decide {
            review: 91.into(),
            suggestion: second,
            decision: Some(crate::Decision::Declined),
        },
        1,
    );
    let r = w.review(91).unwrap().clone();
    let undecided_other = r.undecided();
    assert_eq!(undecided_other, 0, "all drafts decided");
    let mut r = r;
    let sent = r.send().unwrap();
    assert!(sent.iter().any(|t| t.starts_with("Eins")));
    assert!(sent[0].contains("```suggestion\nneu();\n```"));
    assert!(!sent.iter().any(|t| t.contains("Zwei")));
}

#[test]
fn a_suggestion_without_new_fields_still_loads() {
    let s: crate::Suggestion = serde_json::from_str(
        r#"{"id":"s1","location":"a.css · Zeile 2","level":"Note","text":"alt"}"#,
    )
    .unwrap();
    assert_eq!((s.path.as_str(), s.line), ("", 0));
    // And the new fields stay out of the saved form while they are unused.
    assert!(!serde_json::to_string(&s).unwrap().contains("side"));
    let _ = AgentComment {
        path: String::new(),
        line: 1,
        start_line: None,
        side: DiffSide::Left,
        text: String::new(),
        suggestion: None,
    };
}

fn review_choice(w: &Workspace) -> crate::language::LangChoice {
    w.runs
        .iter()
        .find(|r| r.key() == key())
        .and_then(|r| r.options.review_language)
        .expect("the review run records its language")
}

#[test]
fn the_review_language_follows_the_order_of_the_rules() {
    use crate::language::LangSource;
    use cockpit_i18n::Lang;
    // Nothing detectable in the sample text of #91 is not guaranteed, so the PR is set to be neutral first.
    let neutral = |w: &mut Workspace| {
        let r = w.reviews.iter_mut().find(|r| r.number == 91).unwrap();
        r.title = "Fix".into();
        r.description = String::new();
        w.items.iter_mut().for_each(|i| i.description.clear());
    };
    // The user's switch wins.
    let mut w = ws();
    neutral(&mut w);
    ok(
        &mut w,
        Action::StartPrReview {
            review: 91.into(),
            language: Some(Lang::De),
            repo_language: Some(Lang::En),
        },
        1,
    );
    assert_eq!(review_choice(&w).lang, Lang::De);
    assert_eq!(review_choice(&w).source, LangSource::Chosen);
    // The repo's language when nothing can be detected.
    let mut w = ws();
    neutral(&mut w);
    ok(
        &mut w,
        Action::StartPrReview {
            review: 91.into(),
            language: None,
            repo_language: Some(Lang::De),
        },
        1,
    );
    assert_eq!(review_choice(&w).source, LangSource::Repo);
    assert_eq!(review_choice(&w).lang, Lang::De);
    // English otherwise.
    let mut w = ws();
    neutral(&mut w);
    ok(
        &mut w,
        Action::StartPrReview {
            review: 91.into(),
            language: None,
            repo_language: None,
        },
        1,
    );
    assert_eq!(review_choice(&w).lang, Lang::En);
    assert_eq!(review_choice(&w).source, LangSource::Default);
    // A description that is clearly English beats the repo.
    let mut w = ws();
    neutral(&mut w);
    {
        let r = w.reviews.iter_mut().find(|r| r.number == 91).unwrap();
        r.description =
            "This change fixes the list that is empty when the user has no access to the project."
                .into();
    }
    ok(
        &mut w,
        Action::StartPrReview {
            review: 91.into(),
            language: None,
            repo_language: Some(Lang::De),
        },
        1,
    );
    assert_eq!(review_choice(&w).source, LangSource::PrText);
    assert_eq!(review_choice(&w).lang, Lang::En);
}
