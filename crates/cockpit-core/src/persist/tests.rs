use super::*;
use crate::{AgentOptions, Decision, RunState, Verdict, agent::STEPS};

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn revision_roundtrip_and_load_still_reads_it() {
    let ws = demo::workspace();
    let text = save_with_revision(&ws, 42);
    assert_eq!(load_with_revision(&text), Ok((ws.clone(), 42)));
    // Whoever does not know the revision reads the same text.
    assert_eq!(load(&text), Ok(ws.clone()));
    // Without a revision in the text: 0.
    assert_eq!(load_with_revision(&save(&ws)), Ok((ws, 0)));
}

#[test]
fn demo_workspace_roundtrip() {
    let ws = demo::workspace();
    assert_eq!(load(&save(&ws)), Ok(ws));
}

#[test]
fn changed_state_survives() {
    let mut ws = demo::workspace();
    let id = ws.items[0].id.clone();
    ws.mark_done(&id);
    let number = ws.reviews[0].number;
    let suggestion = ws.reviews[0].suggestions[0].id.clone();
    ws.decide(number, &suggestion, Some(Decision::Approved));
    ws.set_verdict(number, Verdict::Approve);
    let back = load(&save(&ws)).unwrap();
    assert!(back.item(&id).unwrap().read);
    assert_eq!(back.review(number).unwrap().verdict, Verdict::Approve);
    assert_eq!(back, ws);
}

#[test]
fn running_agent_continues_at_its_step() {
    let mut ws = demo::workspace();
    let key = ws.items.iter().find(|i| i.is_ticket()).unwrap().key.clone();
    let direct = AgentOptions {
        plan_first: false,
        ..AgentOptions::default()
    };
    ws.start_agent(&key, direct).unwrap();
    crate::test_support::advance(&mut ws);
    crate::test_support::advance(&mut ws);
    let mut back = load(&save(&ws)).unwrap();
    assert_eq!(back.running_agents(), 1);
    let run = back.runs.iter().find(|r| r.is_running()).unwrap();
    assert_eq!((run.completed, run.state), (2, RunState::Running));
    // … and runs to the end after loading.
    for _ in 0..STEPS {
        crate::test_support::advance(&mut back);
    }
    assert_eq!(back.running_agents(), 0);
    assert!(back.runs.iter().any(|r| r.key() == key && r.is_done()));
}

#[test]
fn features_of_other_modules_survive() {
    let mut ws = demo::workspace();
    // Own line comment in the review …
    ws.add_line_comment(87, "components.css", 2, "Bitte Token verwenden.", None)
        .unwrap();
    // … and an agent whose plan waits for approval, with a note.
    ws.start_agent("BAU-142", AgentOptions::default()).unwrap();
    assert!(ws.revise_plan("BAU-142", "Auch den Slider prüfen"));
    let back = load(&save(&ws)).unwrap();
    assert_eq!(back, ws);
    assert_eq!(back.review(87).unwrap().line_comments.len(), 1);
    let run = back
        .runs
        .iter()
        .find(|r| r.ticket_key == "BAU-142")
        .unwrap();
    assert_eq!(run.state, RunState::AwaitingApproval);
    assert_eq!(run.plan_notes, vec!["Auch den Slider prüfen".to_string()]);
}

#[test]
fn garbage_is_unreadable() {
    for text in [
        "",
        "kaputt",
        "{",
        "null",
        "[]",
        "{\"version\":1}",
        "{\"version\":1,\"workspace\":5}",
    ] {
        assert_eq!(load(text), Err(LoadError::Unreadable), "{text:?}");
    }
}

#[test]
fn other_version_is_rejected() {
    let mut value = json(&save(&demo::workspace()));
    value["version"] = (VERSION + 1).into();
    assert_eq!(
        load(&value.to_string()),
        Err(LoadError::Version(VERSION + 1))
    );
    // Even if the rest no longer fits today's shape.
    assert_eq!(
        load("{\"version\":0,\"workspace\":\"alt\"}"),
        Err(LoadError::Version(0))
    );
}

#[test]
fn missing_fields_get_defaults() {
    let mut value = json(&save(&demo::workspace()));
    let review = &mut value["workspace"]["reviews"][0];
    let number = review["number"].as_u64().unwrap() as u32;
    review["sent"] = true.into();
    review.as_object_mut().unwrap().remove("verdict");
    review.as_object_mut().unwrap().remove("suggestions");
    review.as_object_mut().unwrap().remove("sent");
    let item = &mut value["workspace"]["items"][0];
    item.as_object_mut().unwrap().remove("read");
    item.as_object_mut().unwrap().remove("comments");
    // Unknown fields from a newer app do not disturb.
    item["etwas_neues"] = "x".into();

    let ws = load(&value.to_string()).unwrap();
    let review = ws.review(number).unwrap();
    assert_eq!(review.verdict, Verdict::default());
    assert!(review.suggestions.is_empty());
    assert!(!review.sent);
    assert!(!ws.items[0].read);
    assert!(ws.items[0].comments.is_empty());
}

#[test]
fn missing_agent_options_get_defaults() {
    let mut ws = demo::workspace();
    let key = ws.items.iter().find(|i| i.is_ticket()).unwrap().key.clone();
    ws.start_agent(&key, AgentOptions::default()).unwrap();
    let mut value = json(&save(&ws));
    let run = &mut value["workspace"]["runs"][0];
    run.as_object_mut().unwrap().remove("options");
    run["options"] = json("{\"hint\":\"nur ein Hinweis\"}");
    let back = load(&value.to_string()).unwrap();
    let options = &back.runs[0].options;
    assert_eq!(options.hint, "nur ein Hinweis");
    assert_eq!(options.base_branch, "main");
    assert!(options.run_tests);
}

#[test]
fn restore_falls_back_to_demo() {
    let demo = demo::workspace();
    assert_eq!(restore(None), (demo.clone(), false));
    assert_eq!(restore(Some("kaputt")), (demo.clone(), false));
    assert_eq!(restore(Some("{\"version\":99}")), (demo.clone(), false));
    let mut changed = demo.clone();
    let id = changed.items[0].id.clone();
    changed.mark_done(&id);
    let (back, restored) = restore(Some(&save(&changed)));
    assert!(restored);
    assert_eq!(back, changed);
}

/// Collects all field names of a JSON value.
fn keys(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (name, inner) in map {
                out.push(name.to_lowercase());
                keys(inner, out);
            }
        }
        serde_json::Value::Array(list) => list.iter().for_each(|v| keys(v, out)),
        _ => {}
    }
}

#[test]
fn no_secret_fields_in_saved_text() {
    // The workspace has no fields for keys, tokens or passwords.
    let mut ws = demo::workspace();
    let key = ws.items.iter().find(|i| i.is_ticket()).unwrap().key.clone();
    ws.start_agent(&key, AgentOptions::default()).unwrap();
    let mut names = Vec::new();
    keys(&json(&save(&ws)), &mut names);
    assert!(names.len() > 20);
    for name in names {
        // Usage counters (language-model tokens) are not credentials.
        if name == "input_tokens" || name == "output_tokens" {
            continue;
        }
        for word in ["token", "secret", "password", "passwort", "api"] {
            assert!(!name.contains(word), "Feld {name}");
        }
    }
}

#[test]
fn question_and_pause_survive_and_old_runs_still_load() {
    let mut ws = demo::workspace();
    ws.start_agent(
        "BAU-142",
        AgentOptions {
            plan_first: false,
            ..AgentOptions::default()
        },
    )
    .unwrap();
    crate::test_support::advance(&mut ws);
    crate::test_support::advance(&mut ws);
    crate::test_support::question(&mut ws, "BAU-142", "Remove it or mark it as deprecated?");
    let back = load(&save(&ws)).unwrap();
    assert_eq!(back, ws);
    let run = back.run("BAU-142").unwrap();
    assert_eq!(run.state, RunState::AwaitingAnswer);
    assert!(run.question.is_some() && run.asked);
    // After loading, one can answer and pause.
    let mut back = back;
    assert!(back.answer_agent("BAU-142", "Entfernen"));
    assert!(back.pause_agent("BAU-142"));
    assert_eq!(
        load(&save(&back)).unwrap().run("BAU-142").unwrap().state,
        RunState::Paused
    );

    // An older state does not know history and question yet.
    let mut value = json(&save(&demo::workspace()));
    let run = &mut value["workspace"]["runs"][0];
    for field in ["messages", "question", "asked", "pending_hints"] {
        run.as_object_mut().unwrap().remove(field);
    }
    let old = load(&value.to_string()).unwrap();
    let run = &old.runs[0];
    assert!(run.messages.is_empty() && run.question.is_none() && !run.asked);
    assert!(run.is_done());
}
