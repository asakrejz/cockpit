//! Speichern und Laden des [`Workspace`] als Text (JSON) – ohne Browser und ohne UI.
//!
//! Die App legt den Text in `localStorage` ab; hier steht nur, wie er aussieht und wie
//! er sicher wieder eingelesen wird:
//!
//! * Der Text trägt eine **Versionsnummer** ([`VERSION`]). Wer die Form der Daten
//!   unverträglich ändert, erhöht sie – alte Stände werden dann verworfen.
//! * **Neue Felder** in den Typen sind unkritisch: Alte Daten ohne das Feld bekommen
//!   den Standardwert (`#[serde(default)]`), unbekannte Felder werden ignoriert. Neue
//!   Felder bekommen deshalb dieses Attribut, sofern ihr Typ `Default` kann.
//! * **Unlesbare Daten** führen nie zum Absturz: [`restore`] fällt auf die Beispieldaten zurück.
//!
//! Schlüssel und Tokens sind nicht Teil des Workspace und landen hier nie.

use serde::{Deserialize, Serialize};

use crate::{Workspace, demo};

/// Version der Speicherform. Bei unverträglichen Änderungen am Datenmodell erhöhen.
pub const VERSION: u32 = 1;

/// Nur die Versionsnummer – damit sie auch gelesen werden kann, wenn der Rest nicht passt.
#[derive(Deserialize)]
struct Header {
    version: u32,
}

#[derive(Serialize)]
struct Out<'a> {
    version: u32,
    workspace: &'a Workspace,
    /// Nur im Server-Modus: Nummer des Stands (zählt mit jeder angewendeten Aktion).
    #[serde(skip_serializing_if = "Option::is_none")]
    revision: Option<u64>,
}

#[derive(Deserialize)]
struct In {
    workspace: Workspace,
    #[serde(default)]
    revision: u64,
}

/// Warum gespeicherte Daten nicht verwendet werden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// Kein gültiges JSON oder nicht in der erwarteten Form.
    Unreadable,
    /// Stammt aus einer anderen Version der App.
    Version(u32),
}

/// Wandelt den Workspace in Text um.
pub fn save(workspace: &Workspace) -> String {
    to_text(workspace, None)
}

/// Wie [`save`], mit der Revisionsnummer des Servers. Dieselbe Form, nur ein Feld mehr;
/// [`load`] liest den Text weiterhin.
pub fn save_with_revision(workspace: &Workspace, revision: u64) -> String {
    to_text(workspace, Some(revision))
}

fn to_text(workspace: &Workspace, revision: Option<u64>) -> String {
    serde_json::to_string(&Out {
        version: VERSION,
        workspace,
        revision,
    })
    // Der Workspace enthält nur Texte, Zahlen und Aufzählungen – das scheitert nicht.
    .unwrap_or_default()
}

/// Liest gespeicherten Text. Prüft zuerst die Version, dann den Inhalt.
pub fn load(text: &str) -> Result<Workspace, LoadError> {
    load_with_revision(text).map(|(workspace, _)| workspace)
}

/// Wie [`load`], dazu die Revisionsnummer (0, wenn der Text keine enthält).
pub fn load_with_revision(text: &str) -> Result<(Workspace, u64), LoadError> {
    let header: Header = serde_json::from_str(text).map_err(|_| LoadError::Unreadable)?;
    if header.version != VERSION {
        return Err(LoadError::Version(header.version));
    }
    serde_json::from_str::<In>(text)
        .map(|saved| (saved.workspace, saved.revision))
        .map_err(|_| LoadError::Unreadable)
}

/// Der Workspace für den Start: der gespeicherte Stand, sonst die Beispieldaten.
/// Der zweite Wert sagt, ob der gespeicherte Stand verwendet wurde.
pub fn restore(saved: Option<&str>) -> (Workspace, bool) {
    match saved.map(load) {
        Some(Ok(workspace)) => (workspace, true),
        _ => (demo::workspace(), false),
    }
}

#[cfg(test)]
mod tests {
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
        // Wer die Revision nicht kennt, liest denselben Text.
        assert_eq!(load(&text), Ok(ws.clone()));
        // Ohne Revision im Text: 0.
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
        ws.advance_agents();
        ws.advance_agents();
        let mut back = load(&save(&ws)).unwrap();
        assert_eq!(back.running_agents(), 1);
        let run = back.runs.iter().find(|r| r.is_running()).unwrap();
        assert_eq!((run.completed, run.state), (2, RunState::Running));
        // … und läuft nach dem Laden zu Ende, mit neuem Pull Request.
        for _ in 0..STEPS {
            back.advance_agents();
        }
        assert_eq!(back.running_agents(), 0);
        assert!(
            back.runs
                .iter()
                .any(|r| r.is_done() && r.pr_number.is_some())
        );
    }

    #[test]
    fn features_of_other_modules_survive() {
        let mut ws = demo::workspace();
        // Eigener Zeilenkommentar im Review …
        ws.add_line_comment(87, "components.css", 2, "Bitte Token verwenden.", None)
            .unwrap();
        // … und ein Agent, dessen Plan auf Freigabe wartet, mit Anmerkung.
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
        // Auch wenn der Rest nicht mehr zur heutigen Form passt.
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
        // Unbekannte Felder aus einer neueren App stören nicht.
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

    /// Sammelt alle Feldnamen eines JSON-Werts.
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
        // Der Workspace kennt keine Felder für Schlüssel, Tokens oder Passwörter.
        let mut ws = demo::workspace();
        let key = ws.items.iter().find(|i| i.is_ticket()).unwrap().key.clone();
        ws.start_agent(&key, AgentOptions::default()).unwrap();
        let mut names = Vec::new();
        keys(&json(&save(&ws)), &mut names);
        assert!(names.len() > 20);
        for name in names {
            // Zähler des Verbrauchs (Sprachmodell-Tokens) sind keine Zugangsdaten.
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
        for _ in 0..3 {
            ws.advance_agents();
        }
        let back = load(&save(&ws)).unwrap();
        assert_eq!(back, ws);
        let run = back.run("BAU-142").unwrap();
        assert_eq!(run.state, RunState::AwaitingAnswer);
        assert!(run.question.is_some() && run.asked);
        // Nach dem Laden lässt sich antworten und pausieren.
        let mut back = back;
        assert!(back.answer_agent("BAU-142", "Entfernen"));
        assert!(back.pause_agent("BAU-142"));
        assert_eq!(
            load(&save(&back)).unwrap().run("BAU-142").unwrap().state,
            RunState::Paused
        );

        // Ein älterer Stand kennt Verlauf und Frage noch nicht.
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
}
