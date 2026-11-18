//! The tickets and pull requests of the sample data.

use super::*;

/// BAU-142, #87, BAU-138, #85, BAU-150, DOC-7 and #12, in this order.
pub(super) fn items() -> [Item; 7] {
    let rich_description = rich::bau142_description();
    let bau142 = Item {
        description: crate::adf::plain_text(&rich_description),
        status: TicketStatus::InProgress,
        labels: vec!["bug".into(), "a11y".into()],

        description_rich: Some(rich_description),
        comments: vec![Comment {
            rich: Some(rich::lena_comment()),
            ..Comment::new(
                lena(),
                "vor 12 Min.",
                "@AS Betrifft auch den Slider-Thumb. Kannst du das gleich mit prüfen?",
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
