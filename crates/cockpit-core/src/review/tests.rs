use super::LineKind::{Add, Context, Del};
use super::suggest::{has_fixed_px, has_hex_color};
use super::*;

fn review() -> Review {
    Review {
        repo: String::new(),
        number: 1,
        title: "Titel".into(),
        description: String::new(),
        author: Person::new("Jonas", "JB"),
        branch: "feature/x".into(),
        base: "main".into(),
        ticket_key: None,
        checks: Checks::Passed,
        files: vec![FileDiff::new(
            "a.css",
            &[
                (1, Context, "x {"),
                (2, Del, "  a: 1;"),
                (2, Add, "  color: #ffffff;"),
                (3, Add, "  outline: none;"),
            ],
        )],
        agent_reviewed: true,
        agent_summary: String::new(),
        suggestions: vec![
            Suggestion::new("a", "a.css · Zeile 2", Level::Note, "Erster"),
            Suggestion::new("b", "a.css · Zeile 3", Level::Important, "Zweiter"),
        ],
        line_comments: Vec::new(),
        verdict: Verdict::Comment,
        sent: false,
        check_runs: Vec::new(),
        merged: None,
    }
}

#[test]
fn stat_counts_added_and_removed_lines() {
    assert_eq!(review().files[0].stat(), "+2 −1");
}

#[test]
fn nothing_is_sent_while_a_suggestion_is_undecided() {
    let mut r = review();
    r.decide("a", Some(Decision::Approved));
    assert!(!r.can_send());
    assert_eq!(r.send(), Err(SendError::Undecided(1)));
    assert!(!r.sent);
    assert_eq!(r.summary(), "1 freigegeben · 0 abgelehnt · 1 offen");
}

#[test]
fn only_approved_comments_are_sent() {
    let mut r = review();
    r.decide("a", Some(Decision::Approved));
    r.decide("b", Some(Decision::Declined));
    assert_eq!(r.send_label(), "Review senden · 1 Kommentar");
    assert_eq!(r.send(), Ok(vec!["Erster".to_string()]));
    assert_eq!(r.send_label(), "Gesendet");
    assert_eq!(r.send(), Err(SendError::AlreadySent));
}

#[test]
fn a_review_without_suggestions_can_be_sent() {
    let mut r = review();
    r.suggestions.clear();
    assert_eq!(r.send_label(), "Review senden · 0 Kommentare");
    assert_eq!(r.send(), Ok(vec![]));
}

#[test]
fn edited_text_is_what_gets_sent() {
    let mut r = review();
    assert!(!r.edit("a", "   "), "leerer Text");
    assert!(!r.edit("unbekannt", "x"));
    assert!(r.edit("a", "  Besser so.  "));
    r.decide("a", Some(Decision::Approved));
    r.decide("b", Some(Decision::Declined));
    assert_eq!(r.send(), Ok(vec!["Besser so.".to_string()]));
}

#[test]
fn any_change_reopens_a_sent_review() {
    let mut r = review();
    r.decide("a", Some(Decision::Approved));
    r.decide("b", Some(Decision::Approved));
    r.send().unwrap();
    r.decide("b", None);
    assert!(!r.sent);
    assert_eq!(r.undecided(), 1);

    r.decide("b", Some(Decision::Declined));
    r.send().unwrap();
    r.set_verdict(Verdict::Approve);
    assert!(!r.sent);
}

#[test]
fn agent_review_finds_hard_coded_values() {
    let mut r = review();
    assert_eq!(r.run_agent_review(), 2);
    assert_eq!(r.suggestions[0].location, "a.css · Zeile 2");
    assert_eq!(r.suggestions[0].level, Level::Note);
    assert_eq!(r.suggestions[1].level, Level::Important);
    assert!(r.suggestions.iter().all(|s| s.decision.is_none()));
}

#[test]
fn hex_colors() {
    assert!(has_hex_color("background: #fff;"));
    assert!(has_hex_color("x #12abCD y"));
    assert!(!has_hex_color("PR #9 ist offen"));
    assert!(!has_hex_color("kein Wert"));
}

#[test]
fn fixed_pixel_values() {
    assert!(has_fixed_px("  left: 46px;"));
    assert!(
        !has_fixed_px("  --comp-slider-thumb-w: 38px;"),
        "tokens may set values"
    );
    assert!(!has_fixed_px("  left: var(--sep-inset);"));
    assert!(!has_fixed_px("  display: flex; /* px */"));
}

#[test]
fn verdict_ids() {
    for (id, label) in Verdict::OPTIONS {
        let v = Verdict::from_id(id).unwrap();
        assert_eq!((v.id(), v.label()), (*id, *label));
    }
}

#[test]
fn line_comments_are_added_trimmed_and_found_per_line() {
    let mut r = review();
    assert_eq!(
        r.add_line_comment("a.css", 3, "  Neu  ", None),
        Some("c1".into())
    );
    assert_eq!(
        r.add_line_comment("a.css", 3, "Noch einer", None),
        Some("c2".into())
    );
    assert_eq!(r.line_comments_at("a.css", 3).len(), 2);
    assert_eq!(r.line_comments_at("a.css", 3)[0].text, "Neu");
    assert!(r.line_comments_at("a.css", 2).is_empty());
    let c = r.line_comments[0].clone();
    assert_eq!(r.line_location(&c).as_deref(), Some("a.css · Zeile 3"));
}

#[test]
fn invalid_line_comments_are_rejected() {
    let mut r = review();
    assert_eq!(r.add_line_comment("a.css", 0, "  ", None), None);
    assert_eq!(r.add_line_comment("b.css", 0, "x", None), None);
    assert_eq!(r.add_line_comment("a.css", 4, "x", None), None);
    assert!(r.line_comments.is_empty());
}

#[test]
fn line_comments_can_be_edited_and_deleted() {
    let mut r = review();
    let id = r.add_line_comment("a.css", 1, "Alt", None).unwrap();
    assert!(!r.edit_line_comment(&id, "   ", None));
    assert!(!r.edit_line_comment("nope", "x", None));
    assert!(r.edit_line_comment(&id, " Neu ", None));
    assert_eq!(r.line_comments[0].text, "Neu");
    assert!(!r.delete_line_comment("nope"));
    assert!(r.delete_line_comment(&id));
    assert!(r.line_comments.is_empty());
    // The ID of a deleted line is not assigned twice.
    let a = r.add_line_comment("a.css", 1, "A", None).unwrap();
    let b = r.add_line_comment("a.css", 1, "B", None).unwrap();
    r.delete_line_comment(&a);
    assert_ne!(r.add_line_comment("a.css", 1, "C", None).unwrap(), b);
}

#[test]
fn own_comments_are_sent_and_counted_without_approval() {
    let mut r = review();
    r.add_line_comment("a.css", 2, "Eigener", None);
    assert_eq!(r.comment_count(), 1);
    assert_eq!(r.send_label(), "Review senden · 1 Kommentar");
    // The rule stays: first decide on all suggestions.
    assert_eq!(r.send(), Err(SendError::Undecided(2)));
    r.decide("a", Some(Decision::Approved));
    r.decide("b", Some(Decision::Declined));
    assert_eq!(r.send_label(), "Review senden · 2 Kommentare");
    assert_eq!(
        r.send(),
        Ok(vec!["Erster".to_string(), "Eigener".to_string()])
    );
    assert_eq!(r.send_label(), "Gesendet");
}

#[test]
fn a_review_without_suggestions_can_be_sent_with_only_own_comments() {
    let mut r = review();
    r.suggestions.clear();
    r.add_line_comment("a.css", 0, "Nur ich", None);
    assert!(r.can_send());
    assert_eq!(r.send(), Ok(vec!["Nur ich".to_string()]));
}

#[test]
fn changing_line_comments_reopens_a_sent_review() {
    let mut r = review();
    r.suggestions.clear();
    r.send().unwrap();
    assert!(r.sent);
    let id = r.add_line_comment("a.css", 0, "Spät", None).unwrap();
    assert!(!r.sent);
    r.send().unwrap();
    assert!(r.edit_line_comment(&id, "Geändert", None));
    assert!(!r.sent);
    r.send().unwrap();
    assert!(r.delete_line_comment(&id));
    assert!(!r.sent);
    // Failed changes open nothing.
    r.send().unwrap();
    assert!(!r.edit_line_comment("nope", "x", None) && !r.delete_line_comment("nope"));
    assert!(r.sent);
}

// ---------- Change suggestions ----------

#[test]
fn a_comment_can_carry_a_suggestion() {
    let mut r = review();
    // Line 2 (position 2) is new: `  color: #ffffff;`
    let id = r
        .add_line_comment("a.css", 2, "Token nehmen", Some("  color: var(--x);\n"))
        .unwrap();
    let c = r.line_comments[0].clone();
    assert_eq!(c.id, id);
    assert_eq!(c.suggestion.as_deref(), Some("  color: var(--x);"));
    let diff = r.suggestion_diff(&c).unwrap();
    assert_eq!(diff.no, 2);
    assert_eq!(diff.before, "  color: #ffffff;");
    assert_eq!(diff.after, vec!["  color: var(--x);".to_string()]);
    assert_eq!(
        c.body(),
        "Token nehmen\n\n```suggestion\n  color: var(--x);\n```"
    );
}

#[test]
fn a_suggestion_may_replace_one_line_with_several() {
    let mut r = review();
    r.add_line_comment("a.css", 3, "", Some("  a: 1;\n  b: 2;"))
        .unwrap();
    let c = r.line_comments[0].clone();
    assert_eq!(r.suggestion_diff(&c).unwrap().after.len(), 2);
    assert_eq!(c.body(), "```suggestion\n  a: 1;\n  b: 2;\n```");
}

#[test]
fn invalid_suggestions_are_rejected() {
    let mut r = review();
    // Nothing to say.
    assert_eq!(r.add_line_comment("a.css", 2, "  ", Some("  \n")), None);
    // Removed line (position 1): nothing can be replaced there.
    assert_eq!(r.add_line_comment("a.css", 1, "x", Some("neu")), None);
    // Same content: no suggestion.
    assert_eq!(
        r.add_line_comment("a.css", 3, "x", Some("  outline: none;")),
        None
    );
    assert!(r.line_comments.is_empty());
    // Only a suggestion, without text, is enough.
    assert!(r.add_line_comment("a.css", 3, "", Some("x")).is_some());
}

#[test]
fn suggestions_can_be_changed_and_removed() {
    let mut r = review();
    let id = r.add_line_comment("a.css", 2, "T", Some("a")).unwrap();
    assert!(r.edit_line_comment(&id, "T", Some("b")));
    assert_eq!(r.line_comments[0].suggestion.as_deref(), Some("b"));
    assert!(r.edit_line_comment(&id, "T", None));
    assert_eq!(r.line_comments[0].suggestion, None);
    // Without text and without suggestion nothing is left.
    assert!(!r.edit_line_comment(&id, "", None));
    assert!(!r.edit_line_comment(&id, "T", Some("  color: #ffffff;")));
    assert_eq!(r.line_comments[0].text, "T");
}

#[test]
fn the_suggestion_is_sent_as_a_suggestion_block() {
    let mut r = review();
    r.suggestions.clear();
    r.add_line_comment("a.css", 2, "Bitte so", Some("  color: red;"))
        .unwrap();
    let sent = r.send().unwrap();
    assert_eq!(sent, vec!["Bitte so\n\n```suggestion\n  color: red;\n```"]);
}

#[test]
fn old_line_comments_without_the_field_still_load() {
    let c: LineComment =
        serde_json::from_str(r#"{"id":"c1","file":"a","line":0,"text":"x"}"#).unwrap();
    assert_eq!(c.suggestion, None);
}

// ---------- Checks ----------

fn with_checks(r: &mut Review) {
    r.check_runs = vec![
        CheckRun::new("Build", Checks::Passed, 95, "ok"),
        CheckRun::new("Lint", Checks::Failed, 12, "error").rerun_as(Checks::Failed),
        CheckRun::new("Tests", Checks::Failed, 40, "flaky"),
    ];
    r.sync_checks();
}

#[test]
fn the_overall_state_follows_the_single_checks() {
    let mut r = review();
    with_checks(&mut r);
    assert_eq!(r.checks, Checks::Failed);
    assert_eq!(r.check_runs[0].duration_label(), "1:35 Min.");
    assert_eq!(r.check_runs[1].duration_label(), "12 Sek.");
}

#[test]
fn only_a_failed_check_can_be_restarted() {
    let mut r = review();
    with_checks(&mut r);
    assert!(!r.restart_check("Build"), "green");
    assert!(!r.restart_check("gibt es nicht"));
    assert!(r.restart_check("Tests"));
    assert!(!r.restart_check("Tests"), "already running");
    assert_eq!(r.check_runs[2].status, Checks::Running);
    assert_eq!(r.check_runs[2].duration_label(), "läuft");
    // Lint is still red, so the overall state stays red.
    assert_eq!(r.checks, Checks::Failed);
}

#[test]
fn a_restarted_check_finishes_after_a_few_ticks_deterministically() {
    let mut r = review();
    with_checks(&mut r);
    r.restart_check("Tests");
    r.restart_check("Lint");
    for _ in 0..CHECK_STEPS - 1 {
        assert!(r.advance_checks().is_empty());
        assert_eq!(
            r.checks,
            Checks::Running,
            "only a finished check counts as red"
        );
    }
    assert_eq!(r.advance_checks(), vec!["Lint", "Tests"]);
    assert_eq!(r.check_runs[2].status, Checks::Passed);
    assert!(r.check_runs[2].log.contains("erfolgreich"));
    assert_eq!(r.check_runs[1].status, Checks::Failed);
    assert_eq!(r.checks, Checks::Failed);
    assert_eq!(r.running_checks(), 0);
    assert!(r.advance_checks().is_empty());
    // The real error can be retried, but stays red.
    assert!(r.restart_check("Lint"));
}

#[test]
fn all_green_after_restarts_makes_the_review_green() {
    let mut r = review();
    r.check_runs = vec![
        CheckRun::new("Build", Checks::Passed, 5, ""),
        CheckRun::new("Tests", Checks::Failed, 5, ""),
    ];
    r.restart_check("Tests");
    (0..CHECK_STEPS).for_each(|_| {
        r.advance_checks();
    });
    assert_eq!(r.checks, Checks::Passed);
}

// ---------- Merging ----------

#[test]
fn merging_needs_green_checks_a_sent_review_and_no_requested_changes() {
    use MergeBlocker::*;
    let ok = |c, sent, v| merge_blockers(c, sent, v, false);
    assert_eq!(ok(Checks::Passed, true, Verdict::Approve), vec![]);
    assert_eq!(ok(Checks::Passed, true, Verdict::Comment), vec![]);
    assert_eq!(
        ok(Checks::Failed, true, Verdict::Approve),
        vec![ChecksFailed]
    );
    assert_eq!(
        ok(Checks::Running, true, Verdict::Approve),
        vec![ChecksRunning]
    );
    assert_eq!(
        ok(Checks::Passed, false, Verdict::Approve),
        vec![ReviewNotSent]
    );
    assert_eq!(
        ok(Checks::Passed, true, Verdict::RequestChanges),
        vec![ChangesRequested]
    );
    assert_eq!(
        ok(Checks::Failed, false, Verdict::RequestChanges),
        vec![ChecksFailed, ReviewNotSent, ChangesRequested]
    );
    assert_eq!(
        merge_blockers(Checks::Passed, true, Verdict::Approve, true),
        vec![AlreadyMerged]
    );
}

#[test]
fn every_blocker_has_a_german_reason() {
    use MergeBlocker::*;
    for b in [
        AlreadyMerged,
        Draft,
        ChecksFailed,
        ChecksRunning,
        ReviewNotSent,
        ChangesRequested,
    ] {
        assert!(b.reason().ends_with('.'), "{b:?}");
    }
}

#[test]
fn merge_methods_have_ids() {
    for (id, label) in MergeMethod::OPTIONS {
        let m = MergeMethod::from_id(id).unwrap();
        assert_eq!((m.id(), m.label()), (*id, *label));
    }
    assert_eq!(MergeMethod::from_id("x"), None);
}
