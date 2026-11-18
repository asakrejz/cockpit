use super::*;
use crate::review::LineKind::{Add, Context, Del};

fn review() -> Review {
    let mut r = crate::demo::workspace().reviews.remove(0);
    r.suggestions.clear();
    r.files = vec![FileDiff::new(
        "a.rs",
        &[
            (1, Context, "fn a() {"),
            (2, Del, "    alt();"),
            (2, Add, "    neu();"),
            (3, Add, "    mehr();"),
            (4, Context, "}"),
        ],
    )];
    r
}

fn draft(line: u32, side: DiffSide, text: &str) -> AgentComment {
    AgentComment {
        path: "a.rs".into(),
        line,
        start_line: None,
        side,
        text: text.into(),
        suggestion: None,
    }
}

#[test]
fn a_draft_needs_a_line_on_the_right_side_of_the_diff() {
    let mut r = review();
    assert!(r.add_agent_comment(draft(3, DiffSide::Right, "a")));
    // Line 2 exists on both sides (old and new), 4 is context (right only), 1 context.
    assert!(r.add_agent_comment(draft(2, DiffSide::Left, "alt")));
    assert!(!r.add_agent_comment(draft(4, DiffSide::Left, "Kontext gibt es links nicht")));
    assert!(!r.add_agent_comment(draft(9, DiffSide::Right, "keine Zeile")));
    assert!(!r.add_agent_comment(draft(0, DiffSide::Right, "Zeile 0")));
    assert!(!r.add_agent_comment(draft(3, DiffSide::Right, "   ")));
    let mut other = draft(3, DiffSide::Right, "x");
    other.path = "b.rs".into();
    assert!(!r.add_agent_comment(other));
    assert_eq!(r.agent_drafts(), 2);
    let s = &r.suggestions[0];
    assert_eq!(
        (s.id.as_str(), s.location.as_str()),
        ("a1", "a.rs · Zeile 3")
    );
    assert_eq!(s.origin, Origin::Agent);
    assert_eq!(s.decision, None);
}

#[test]
fn ranges_suggestions_and_duplicates() {
    let mut r = review();
    let mut c = draft(3, DiffSide::Right, "Bereich");
    c.start_line = Some(2);
    c.suggestion = Some("    a();\n    b();\n\n".into());
    assert!(r.add_agent_comment(c));
    assert!(
        !r.add_agent_comment(draft(3, DiffSide::Right, "Bereich")),
        "same draft twice"
    );
    let s = &r.suggestions[0];
    assert_eq!(
        (s.start_line, s.code.as_deref()),
        (Some(2), Some("    a();\n    b();"))
    );
    assert_eq!(
        s.body(),
        "Bereich\n\n```suggestion\n    a();\n    b();\n```"
    );
    // A start line that does not exist or lies behind the end is dropped, the draft stays.
    let mut c = draft(3, DiffSide::Right, "kaputter Bereich");
    c.start_line = Some(8);
    assert!(r.add_agent_comment(c));
    assert_eq!(r.suggestions[1].start_line, None);
    // On the left side there is no replacement code.
    let mut c = draft(2, DiffSide::Left, "links");
    c.suggestion = Some("x".into());
    assert!(r.add_agent_comment(c));
    assert_eq!(r.suggestions[2].code, None);
}

#[test]
fn at_most_forty_drafts_and_unique_ids() {
    let mut r = review();
    for i in 0..60 {
        r.add_agent_comment(draft(3, DiffSide::Right, &format!("t{i}")));
    }
    assert_eq!(r.agent_drafts(), MAX_AGENT_COMMENTS);
    let mut ids: Vec<&str> = r.suggestions.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), MAX_AGENT_COMMENTS);
}

#[test]
fn drafts_show_under_their_diff_line_and_can_be_edited_and_declined() {
    let mut r = review();
    r.add_agent_comment(draft(2, DiffSide::Left, "alt"));
    r.add_agent_comment(draft(2, DiffSide::Right, "neu"));
    // Index 1 is the removed line, index 2 the added one.
    assert_eq!(r.suggestions_at("a.rs", 1)[0].text, "alt");
    assert_eq!(r.suggestions_at("a.rs", 2)[0].text, "neu");
    assert!(r.suggestions_at("a.rs", 0).is_empty());
    assert!(r.edit("a2", "besser formuliert"));
    assert_eq!(r.suggestions_at("a.rs", 2)[0].text, "besser formuliert");
    r.decide("a1", Some(Decision::Declined));
    r.decide("a2", Some(Decision::Approved));
    let out = r.send().unwrap();
    assert_eq!(out, vec!["besser formuliert".to_string()]);
}

#[test]
fn a_new_run_clears_old_drafts_and_the_summary() {
    let mut r = review();
    r.add_agent_comment(draft(3, DiffSide::Right, "a"));
    r.set_agent_summary("  Zusammenfassung  ");
    assert_eq!(r.agent_summary, "Zusammenfassung");
    assert_eq!(r.finish_agent_review(), 1);
    assert!(r.agent_reviewed);
    r.begin_agent_review();
    assert!(r.suggestions.is_empty() && r.agent_summary.is_empty() && !r.agent_reviewed);
}
