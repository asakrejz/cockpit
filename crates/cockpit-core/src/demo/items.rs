//! The tickets and pull requests of the sample data.

use super::*;
use crate::project::{FieldKind, FieldValue, ProjectItem};

/// BAU-142, #87, BAU-138, #85, BAU-150, DOC-7 and #12, in this order.
pub(super) fn items() -> [Item; 7] {
    let mut rich_description = rich::bau142_description();
    let details = rich::bau142_details();
    crate::details::link_media(&mut rich_description, &details.attachments);
    let bau142 = Item {
        details: Some(details),
        url: Some("https://acme.atlassian.net/browse/BAU-142".into()),
        description: crate::adf::plain_text(&rich_description),
        status: TicketStatus::InProgress,
        labels: vec!["bug".into(), "a11y".into()],

        description_rich: Some(rich_description),
        comments: vec![Comment {
            rich: Some(rich::lena_comment()),
            ..Comment::new(
                lena(),
                "vor 12 Min.",
                "@Du Betrifft auch den Slider-Thumb. Kannst du das gleich mit prüfen?",
            )
        }],
        ..item(
            "bau142",
            Kind::Ticket,
            "BAU-142",
            "Toggle: Fokus-Ring fehlt im Dark Mode",
            "Im Dark Mode ist der Fokus-Ring des Toggles kaum zu erkennen. Erwartet: Der Ring nutzt --sys-focus-ring und bleibt auf dunklem Hintergrund klar sichtbar.",
            (ReasonKind::Comment, "Neuer Kommentar von Lena"),
            "vor 12 Min.",
            Person::me(),
        )
    };
    let pr87 = Item {
        repo: Some(DEMO_REPO.into()),
        labels: vec!["design".into()],
        branch: Some("feature/BAU-131-slider-thumb".into()),
        pr: Some(PrRef {
            number: 87,
            repo: DEMO_REPO.into(),
            draft: false,
            checks: Checks::Passed,
            changes_requested: false,
            merged: false,
            review_requested: true,
            author: String::new(),
            mine: false,
            requested_me: false,
            assigned_me: false,
        }),
        agent_note: Some("3 Kommentare vom Agenten warten auf deine Freigabe".into()),
        agent_note_msg: None,
        comments: vec![Comment::new(
            jonas(),
            "vor 40 Min.",
            "Magst du kurz drüberschauen? Bin mir beim Fokus-Zustand nicht sicher.",
        )],
        ..item(
            "pr87",
            Kind::PullRequest,
            "#87",
            "Slider: Thumb als Kapsel",
            "Stellt den Slider-Thumb auf die Kapselform um und gleicht die Maße an den Toggle an. 2 Dateien geändert.",
            (ReasonKind::ReviewRequested, "Review angefragt von Jonas"),
            "vor 40 Min.",
            jonas(),
        )
    };
    let bau138 = Item {
        status: TicketStatus::Review,
        labels: vec!["a11y".into(), "agent".into()],

        branch: Some("claude/BAU-138-segmented-picker".into()),
        pr: Some(PrRef {
            number: 91,
            repo: DEMO_REPO.into(),
            draft: true,
            checks: Checks::Passed,
            changes_requested: true,
            merged: false,
            review_requested: true,
            author: String::new(),
            mine: false,
            requested_me: false,
            assigned_me: false,
        }),
        agent_note: Some("Vom Agenten umgesetzt · Tests bestanden".into()),
        agent_note_msg: None,
        ..item(
            "bau138",
            Kind::Ticket,
            "BAU-138",
            "Segmented Picker: Tastaturbedienung",
            "Der Segmented Picker soll mit Pfeiltasten bedienbar sein und den gewählten Eintrag für Screenreader ansagen.",
            (
                ReasonKind::AgentFinished,
                "Agent fertig · Pull Request #91 erstellt",
            ),
            "vor 1 Std.",
            Person::me(),
        )
    };
    let pr85 = Item {
        repo: Some(DEMO_REPO.into()),
        labels: vec!["bug".into()],
        branch: Some("feature/BAU-127-list-separator".into()),
        pr: Some(PrRef {
            number: 85,
            repo: DEMO_REPO.into(),
            draft: false,
            checks: Checks::Failed,
            changes_requested: false,
            merged: false,
            review_requested: false,
            author: String::new(),
            mine: false,
            requested_me: false,
            assigned_me: false,
        }),
        comments: vec![Comment::new(
            mira(),
            "vor 2 Std.",
            "Lint meckert über eine ungenutzte Variable – schaue ich mir gleich an.",
        )],
        ..item(
            "pr85",
            Kind::PullRequest,
            "#85",
            "List: Trennlinien-Einzug mit Icons",
            "Rückt die Trennlinie ein, wenn die Liste Icons zeigt. Der Build schlägt im Lint-Schritt fehl.",
            (ReasonKind::ChecksFailed, "Checks fehlgeschlagen"),
            "vor 2 Std.",
            mira(),
        )
    };
    let bau150 = Item {
        // Due before „today“ (11.11.2026): the rule „overdue ticket“ applies.
        due: Some(DEMO_OVERDUE.into()),
        labels: vec!["feature".into()],
        ..item(
            "bau150",
            Kind::Ticket,
            "BAU-150",
            "Sheet: Detents mittel und groß",
            "Das Sheet soll zwei Höhen unterstützen (mittel, groß) und sich per Grabber zwischen ihnen ziehen lassen.",
            (ReasonKind::Assigned, "Dir zugewiesen von Mira"),
            "gestern",
            Person::me(),
        )
    };
    // A second project (DOC) and a second repo, so that the inbox filters show something.
    let doc7 = Item {
        status: TicketStatus::Open,
        labels: vec!["docs".into()],
        ..item(
            "doc7",
            Kind::Ticket,
            "DOC-7",
            "Handbuch: Kapitel zu Tokens ergänzen",
            "Das Handbuch erklärt Referenz-, System- und Komponenten-Tokens bisher nur in einem Absatz.",
            (ReasonKind::Assigned, "Dir zugewiesen von Lena"),
            "vor 3 Std.",
            Person::me(),
        )
    };
    let pr12 = Item {
        repo: Some(DEMO_REPO_TOKENS.into()),
        labels: vec!["docs".into(), "bug".into()],
        branch: Some("fix/DOC-7-token-table".into()),
        pr: Some(PrRef {
            number: 12,
            repo: DEMO_REPO_TOKENS.into(),
            draft: false,
            checks: Checks::Failed,
            changes_requested: false,
            merged: false,
            review_requested: false,
            author: String::new(),
            mine: false,
            requested_me: false,
            assigned_me: false,
        }),
        ..item(
            "pr12",
            Kind::PullRequest,
            "#12",
            "Token-Tabelle: Spalte für Dark Mode",
            "Ergänzt die Token-Tabelle um eine Spalte für den Dark Mode. Der Link-Check schlägt fehl.",
            (ReasonKind::ChecksFailed, "Checks fehlgeschlagen"),
            "vor 4 Std.",
            Person::me(),
        )
    };
    [bau142, pr87, bau138, pr85, bau150, doc7, pr12]
}

/// The fields of the sample GitHub project, as a sync would leave them.
pub fn project_meta() -> crate::project::ProjectMeta {
    use crate::project::{FieldDef, SelectOption};
    let options = |names: &[(&str, &str)]| -> Vec<SelectOption> {
        names
            .iter()
            .map(|(id, name)| SelectOption {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect()
    };
    let field = |id: &str, name: &str, kind, options| FieldDef {
        id: id.into(),
        name: name.into(),
        kind,
        options,
    };
    crate::project::ProjectMeta {
        id: "PVT_demo".into(),
        owner: "acme".into(),
        number: 3,
        title: "App roadmap".into(),
        url: "https://github.com/orgs/acme/projects/3".into(),
        fields: vec![
            field(
                "PVTSSF_status",
                "Status",
                FieldKind::SingleSelect,
                options(&[
                    ("s_todo", "Todo"),
                    ("s_prog", "In Progress"),
                    ("s_rev", "In Review"),
                    ("s_done", "Done"),
                ]),
            ),
            field(
                "PVTSSF_team",
                "Team",
                FieldKind::SingleSelect,
                options(&[("t_core", "Core"), ("t_web", "Web")]),
            ),
            field("PVTF_est", "Estimate", FieldKind::Number, vec![]),
            field("PVTF_due", "Due date", FieldKind::Date, vec![]),
            field("PVTF_notes", "Notes", FieldKind::Text, vec![]),
        ],
        status_field: None,
    }
}

/// The name of the sample GitHub project.
pub const DEMO_PROJECT: &str = "acme/3";

/// Two tickets of a sample GitHub project („acme/3“): an issue in review and a draft. They show
/// the source `GitHubProject`, Markdown text, project fields and a pull request found on the
/// timeline. Nothing here reaches a real project.
pub fn project_tickets() -> (Item, Item) {
    let project = |item_id: &str, number: Option<u32>, draft: bool| ProjectItem {
        project_id: "PVT_demo".into(),
        project: DEMO_PROJECT.into(),
        project_title: "App roadmap".into(),
        item_id: item_id.into(),
        content_id: if draft { "DI_demo_1" } else { "I_demo_31" }.into(),
        draft,
        number,
        status_option: Some(if draft { "s_todo" } else { "s_rev" }.into()),
        ..ProjectItem::default()
    };
    let issue = Item {
        source: Source::GitHubProject,
        repo: Some(DEMO_REPO.into()),
        status: TicketStatus::Review,
        labels: vec!["enhancement".into(), "a11y".into()],
        reporter: Some("Lena".into()),
        issue_type: Some("Feature".into()),
        milestone: Some("v1.4".into()),
        priority: Priority::High,
        team: Some(TeamRef::new("t_core", "Core")),
        due: Some(DEMO_OVERDUE.into()),
        url: Some("https://github.com/acme/app/issues/31".into()),
        description: "Der **Fokusring** fehlt bei Tab-Navigation.\n\n- [x] Reproduziert\n- [ ] Test ergänzen\n\nSiehe `docs/a11y.md`.".into(),
        comments: vec![Comment {
            author: lena(),
            time: "vor 2 Std.".into(),
            created_ms: None,
            text: "Ich habe das mit **VoiceOver** geprüft: der Ring fehlt nur im Dark Mode.".into(),
            text_msg: None,
            rich: None,
            own: false,
        }],
        project: Some(ProjectItem {
            fields: vec![FieldValue {
                field_id: "PVTF_est".into(),
                name: "Estimate".into(),
                kind: FieldKind::Number,
                text: "3".into(),
                option_id: None,
            }],
            assignees: vec!["me".into()],
            milestone: Some("v1.4".into()),
            ..project("PVTI_demo_31", Some(31), false)
        }),
        ..item(
            "gh-acme-app-31",
            Kind::Ticket,
            "acme/app#31",
            "Fokusring bei Tab-Navigation sichtbar machen",
            "",
            (ReasonKind::Assigned, "Dir zugewiesen"),
            "vor 2 Std.",
            Person::me(),
        )
    };
    let draft = Item {
        source: Source::GitHubProject,
        status: TicketStatus::Open,
        reporter: Some("Du".into()),
        description: "Nur eine Idee: **Dark Mode** für die Handbuch-Seiten.".into(),
        project: Some(project("PVTI_demo_draft", None, true)),
        ..item(
            "gh-acme-3-draft1",
            Kind::Ticket,
            "acme/3#draft-demo01",
            "Dark Mode für das Handbuch prüfen",
            "",
            (ReasonKind::Comment, "Im Projekt"),
            "gestern",
            Person::nobody(),
        )
    };
    (issue, draft)
}
