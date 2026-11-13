use super::*;
use crate::github::dto::{CheckOutput, User};

fn comment(id: u64, reply: Option<u64>, who: &str, text: &str, path: &str) -> ReviewComment {
    ReviewComment {
        id,
        in_reply_to_id: reply,
        user: User { login: who.into() },
        body: Some(text.into()),
        created_at: String::new(),
        path: path.into(),
        line: Some(3),
    }
}

#[test]
fn replies_are_grouped_into_the_thread_of_the_first_comment() {
    let all = vec![
        comment(1, None, "mira", "Warum hier unwrap?", "a.rs"),
        comment(2, None, "tom", "Name passt nicht.", "b.rs"),
        comment(3, Some(1), "ich", "Stimmt.", "a.rs"),
        comment(4, Some(3), "mira", "Danke.", "a.rs"),
    ];
    let t = threads(&all);
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].path, "a.rs");
    assert_eq!(
        t[0].comments
            .iter()
            .map(|c| c.author.as_str())
            .collect::<Vec<_>>(),
        ["mira", "ich", "mira"]
    );
    assert_eq!(t[1].comments.len(), 1);
}

#[test]
fn empty_comments_cycles_and_outdated_lines_are_handled() {
    let mut a = comment(1, Some(2), "x", "A", "a.rs");
    let b = comment(2, Some(1), "y", "B", "a.rs");
    a.line = None;
    let t = threads(&[a, b, comment(9, None, "z", "  ", "c.rs")]);
    // The cycle ends (no endless loop); the empty comment is dropped.
    assert!(!t.is_empty() && t.len() <= 2);
    assert!(t.iter().all(|t| t.path == "a.rs"));
    assert_eq!(t[0].line, None);
}

#[test]
fn amounts_and_sizes_are_limited() {
    let many: Vec<ReviewComment> = (1..=100)
        .map(|i| comment(i, None, "r", &format!("Kommentar {i}"), "f.rs"))
        .collect();
    let t = threads(&many);
    assert_eq!(t.len(), MAX_THREADS);
    // The newest ones are kept.
    assert_eq!(t.last().unwrap().comments[0].text, "Kommentar 100");

    let mut long = vec![comment(1, None, "r", &"ä".repeat(10_000), "f.rs")];
    for i in 2..=20 {
        long.push(comment(i, Some(1), "r", "antwort", "f.rs"));
    }
    let t = threads(&long);
    assert_eq!(t[0].comments.len(), MAX_COMMENTS);
    assert_eq!(t[0].comments[0].text.chars().count(), MAX_COMMENT_CHARS);
    assert!(t[0].comments[0].text.ends_with('…'));
}

#[test]
fn only_reviews_with_a_request_and_text_count() {
    let r = |state: &str, body: Option<&str>| PullReview {
        user: User {
            login: "mira".into(),
        },
        state: state.into(),
        body: body.map(Into::into),
        submitted_at: String::new(),
    };
    let n = notes(&[
        r("APPROVED", Some("super")),
        r("CHANGES_REQUESTED", Some("Bitte Tests ergänzen.")),
        r("COMMENTED", Some("   ")),
        r("DISMISSED", Some("alt")),
        r("COMMENTED", Some("Noch eine Frage.")),
    ]);
    assert_eq!(n.len(), 2);
    assert_eq!(n[0].state, "CHANGES_REQUESTED");
}

#[test]
fn the_excerpt_keeps_the_end_and_adds_annotations() {
    let run = CheckRun {
        name: "Lint".into(),
        status: "completed".into(),
        conclusion: Some("failure".into()),
        output: Some(CheckOutput {
            title: Some("2 Fehler".into()),
            summary: Some("x".repeat(5_000)),
            text: Some("ENDE-DES-LOGS".into()),
        }),
        ..Default::default()
    };
    assert!(is_failed(&run));
    let a = Annotation {
        path: "src/a.rs".into(),
        start_line: 7,
        annotation_level: "failure".into(),
        message: "ungenutzte Variable".into(),
    };
    let notice = Annotation {
        annotation_level: "notice".into(),
        message: "nur ein Hinweis".into(),
        ..Default::default()
    };
    let e = excerpt(&run, &[a, notice]);
    assert!(e.chars().count() <= MAX_EXCERPT_CHARS);
    assert!(e.starts_with('…'));
    assert!(e.contains("src/a.rs:7: ungenutzte Variable"));
    assert!(!e.contains("nur ein Hinweis"));
    let ok = CheckRun {
        conclusion: Some("success".into()),
        status: "completed".into(),
        ..Default::default()
    };
    assert!(!is_failed(&ok));
}

#[test]
fn the_overall_budget_drops_the_oldest_threads() {
    let big = "ä".repeat(MAX_COMMENT_CHARS);
    let mut many: Vec<ReviewComment> = Vec::new();
    for i in 0..30u64 {
        let root = i * 10 + 1;
        many.push(comment(root, None, "r", &big, "f.rs"));
        for k in 1..=3 {
            many.push(comment(root + k, Some(root), "r", &big, "f.rs"));
        }
    }
    let t = threads(&many);
    let total: usize = t
        .iter()
        .flat_map(|t| &t.comments)
        .map(|c| c.text.chars().count())
        .sum();
    assert!(total <= MAX_TOTAL_CHARS + 4 * MAX_COMMENT_CHARS, "{total}");
    assert!(t.len() < 30 && !t.is_empty());
}
