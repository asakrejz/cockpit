//! Beispieldaten nach dem Design. Sie stehen für Jira, GitHub und den Agenten,
//! solange kein Backend angebunden ist.

use crate::agent::{AgentRun, Question, RunState, STEPS};
use crate::model::*;
use crate::review::LineKind::{Add, Context as Ctx, Del};
use crate::review::{CheckRun, FileDiff, Level, Review, Suggestion, Verdict};
use crate::workspace::Workspace;

fn lena() -> Person {
    Person::new("Lena", "LK")
}
fn jonas() -> Person {
    Person::new("Jonas", "JB")
}
fn mira() -> Person {
    Person::new("Mira", "MT")
}

#[allow(clippy::too_many_arguments)]
fn item(
    id: &str,
    kind: Kind,
    key: &str,
    title: &str,
    description: &str,
    (reason, reason_text): (ReasonKind, &str),
    time: &str,
    owner: Person,
) -> Item {
    let mine = owner == Person::me();
    Item {
        id: id.into(),
        kind,
        source: if kind == Kind::Ticket {
            Source::Jira
        } else {
            Source::GitHub
        },
        key: key.into(),
        title: title.into(),
        description: description.into(),
        reason,
        reason_text: reason_text.into(),
        time: time.into(),
        scope: if mine { Scope::Mine } else { Scope::Team },
        owner,
        status: TicketStatus::Open,
        branch: None,
        pr: None,
        agent_note: None,
        comments: Vec::new(),
        read: false,
        due: None,
    }
}

/// Fälligkeit des Demo-Tickets BAU-150: liegt vor dem Tag, an dem die Demo entstand.
pub const DEMO_OVERDUE: &str = "2026-11-09";

/// Der Ausgangszustand des Cockpits.
pub fn workspace() -> Workspace {
    let bau142 = Item {
        status: TicketStatus::InProgress,
        comments: vec![Comment::new(
            lena(),
            "vor 12 Min.",
            "@AS Betrifft auch den Slider-Thumb. Kannst du das gleich mit prüfen?",
        )],
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
        branch: Some("feature/BAU-131-slider-thumb".into()),
        pr: Some(PrRef {
            number: 87,
            draft: false,
            checks: Checks::Passed,
            changes_requested: false,
            merged: false,
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
        branch: Some("agent/BAU-138-segmented-picker".into()),
        pr: Some(PrRef {
            number: 91,
            draft: true,
            checks: Checks::Passed,
            changes_requested: true,
            merged: false,
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
        branch: Some("feature/BAU-127-list-separator".into()),
        pr: Some(PrRef {
            number: 85,
            draft: false,
            checks: Checks::Failed,
            changes_requested: false,
            merged: false,
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
        // Fällig vor „heute“ (11.11.2026): greift die Regel „überfälliges Ticket“.
        due: Some(DEMO_OVERDUE.into()),
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

    let review =
        |number: u32, item: &Item, author: Person, ticket: &str, files: Vec<FileDiff>| Review {
            number,
            title: item.title.clone(),
            description: item.description.clone(),
            author,
            branch: item.branch.clone().unwrap_or_default(),
            base: "main".into(),
            ticket_key: Some(ticket.into()),
            checks: item.pr.as_ref().map_or(Checks::Running, |p| p.checks),
            files,
            agent_reviewed: false,
            suggestions: Vec::new(),
            line_comments: Vec::new(),
            verdict: Verdict::Comment,
            sent: false,
            check_runs: if item.pr.as_ref().is_some_and(|p| p.checks == Checks::Passed) {
                passing_checks()
            } else {
                Vec::new()
            },
            merged: None,
        };

    let review87 = Review {
        description:
            "Stellt den Slider-Thumb auf die Kapselform um und gleicht die Maße an den Toggle an."
                .into(),
        agent_reviewed: true,
        suggestions: vec![
            Suggestion::new(
                "a",
                "components.css · Zeile 417",
                Level::Note,
                "Hier steht ein fester Farbwert. Komponenten sollen nur Tokens lesen – Vorschlag: var(--comp-toggle-thumb) oder ein eigenes --comp-slider-thumb.",
            ),
            Suggestion::new(
                "b",
                "components.css · Zeile 418",
                Level::Note,
                "Die Dauer ist fest eingetragen. Mit var(--sys-motion-snappy) greift auch die Einstellung „Bewegung reduzieren“.",
            ),
            Suggestion::new(
                "c",
                "components.css · Zeile 420",
                Level::Important,
                "outline: none entfernt den Fokus-Ring ohne Ersatz. Wer mit der Tastatur bedient, sieht den Fokus nicht mehr – bitte --sys-focus-ring auf den Thumb setzen.",
            ),
        ],
        ..review(
            87,
            &pr87,
            jonas(),
            "BAU-131",
            vec![
                FileDiff::new(
                    "components.css",
                    &[
                        (412, Ctx, ".ui-slider::-webkit-slider-thumb {"),
                        (413, Del, "  width: 28px;"),
                        (414, Del, "  height: 28px;"),
                        (413, Add, "  width: var(--comp-slider-thumb-w);"),
                        (414, Add, "  height: var(--comp-slider-thumb-h);"),
                        (415, Ctx, "  border-radius: var(--ref-radius-full);"),
                        (416, Ctx, "  box-shadow: var(--ref-shadow-thumb);"),
                        (417, Add, "  background: #ffffff;"),
                        (418, Add, "  transition: transform 0.2s ease;"),
                        (419, Ctx, "}"),
                        (420, Add, ".ui-slider:focus-visible { outline: none; }"),
                    ],
                ),
                FileDiff::new(
                    "tokens.css",
                    &[
                        (268, Ctx, "  --comp-slider-fill: var(--sys-color-tint);"),
                        (269, Add, "  --comp-slider-thumb-w: 38px;"),
                        (270, Add, "  --comp-slider-thumb-h: 24px;"),
                    ],
                ),
            ],
        )
    };
    let review91 = Review {
        description:
            "Vom Agenten für BAU-138 umgesetzt · Tests bestanden. Der Agent führt keinen Merge aus."
                .into(),
        ..review(
            91,
            &bau138,
            Person::agent(),
            "BAU-138",
            agent_changes("BAU-138"),
        )
    };
    let mut review85 = review(
        85,
        &pr85,
        mira(),
        "BAU-127",
        vec![
            FileDiff::new(
                "components.css",
                &[
                    (159, Ctx, ".ui-list__group[data-icons] {"),
                    (160, Del, "  --sep-inset: 58px;"),
                    (
                        160,
                        Add,
                        "  --sep-inset: calc(var(--sys-space-row-x) + var(--comp-list-icon-size) + var(--ref-space-12));",
                    ),
                    (161, Ctx, "}"),
                    (
                        162,
                        Add,
                        ".ui-list__group[data-icons] .ui-list__row::before { left: 46px; }",
                    ),
                ],
            ),
            FileDiff::new(
                "list.rs",
                &[
                    (61, Ctx, "    let body = view! {"),
                    (62, Add, "        let inset = icon.map(|_| 46);"),
                    (63, Ctx, "        {icon.map(|(tint, path)| {"),
                ],
            ),
        ],
    );

    review85.check_runs = failing_checks();

    // BAU-138 hat der Agent schon umgesetzt.
    let mut run138 = AgentRun::new(&bau138.key, &bau138.title);
    run138.files = review91.files.iter().map(|f| f.name.clone()).collect();
    run138.state = RunState::Done;
    run138.completed = STEPS;
    run138.pr_number = Some(91);

    Workspace {
        items: vec![bau142, pr87, bau138, pr85, bau150],
        reviews: vec![review87, review91, review85],
        runs: vec![run138],
        log: Vec::new(),
        ..Workspace::default()
    }
}

/// Die Rückfrage, die der Demo-Agent für ein Ticket stellt: vor welchem Schritt (Zahl
/// der abgeschlossenen Schritte) und was. Für alle anderen Tickets fragt er nichts.
pub fn agent_question(ticket_key: &str) -> Option<(usize, Question)> {
    match ticket_key {
        "BAU-142" => Some((
            2,
            Question {
                text: "Soll der alte Stil entfernt oder als deprecated markiert werden?".into(),
                suggestions: vec![
                    "Entfernen".into(),
                    "Als deprecated markieren".into(),
                    "Erst markieren, später entfernen".into(),
                ],
            },
        )),
        _ => None,
    }
}

/// Drei grüne Checks, wie sie ein sauberer Pull Request hat.
pub fn passing_checks() -> Vec<CheckRun> {
    vec![
        CheckRun::new(
            "Build",
            Checks::Passed,
            94,
            "$ trunk build --release\n   Compiling cockpit v0.1.0\n    Finished release [optimized] in 1m 34s\n",
        ),
        CheckRun::new(
            "Lint",
            Checks::Passed,
            41,
            "$ cargo clippy --all-targets -- -D warnings\n    Finished dev [unoptimized] in 41s\n",
        ),
        CheckRun::new(
            "Tests",
            Checks::Passed,
            58,
            "$ cargo test --workspace\ntest result: ok. 131 passed; 0 failed; 0 ignored\n",
        ),
    ]
}

/// Checks von #85: der Lint-Fehler ist echt, der Test wackelt nur.
fn failing_checks() -> Vec<CheckRun> {
    vec![
        CheckRun::new(
            "Build",
            Checks::Passed,
            88,
            "$ trunk build --release\n    Finished release [optimized] in 1m 28s\n",
        ),
        CheckRun::new(
            "Lint",
            Checks::Failed,
            37,
            "$ cargo clippy --all-targets -- -D warnings\nerror: unused variable: `inset`\n  --> features/ui/src/list.rs:62:13\n   |\n62 |         let inset = icon.map(|_| 46);\n   |             ^^^^^ help: if this is intentional, prefix it with an underscore: `_inset`\n   |\n   = note: `-D unused-variables` implied by `-D warnings`\n\nerror: could not compile `feature-ui` (lib) due to 1 previous error\n",
        )
        .rerun_as(Checks::Failed),
        CheckRun::new(
            "Tests (Browser)",
            Checks::Failed,
            124,
            "$ cargo test --target wasm32-unknown-unknown\nrunning 1 test\ntest list::separator_inset_follows_icons ... FAILED\n\nfailures:\n    list::separator_inset_follows_icons\n\nthread panicked: timeout after 120s waiting for the headless browser to start\n\ntest result: FAILED. 0 passed; 1 failed\n",
        ),
    ]
}

/// Was der Demo-Agent für ein Ticket ändert.
pub fn agent_changes(ticket_key: &str) -> Vec<FileDiff> {
    match ticket_key {
        "BAU-142" => vec![
            FileDiff::new(
                "components.css",
                &[
                    (108, Ctx, ".ui-toggle input:focus-visible {"),
                    (109, Ctx, "  outline: none;"),
                    (110, Del, "  box-shadow: 0 0 0 4px rgb(0 122 255 / 0.3);"),
                    (110, Add, "  box-shadow: var(--sys-focus-ring);"),
                    (111, Ctx, "}"),
                    (124, Ctx, ".ui-slider:focus-visible::-webkit-slider-thumb {"),
                    (
                        125,
                        Add,
                        "  box-shadow: var(--ref-shadow-thumb), var(--sys-focus-ring);",
                    ),
                    (126, Ctx, "}"),
                ],
            ),
            FileDiff::new(
                "tokens.css",
                &[
                    (322, Ctx, "[data-theme=\"dark\"] {"),
                    (
                        323,
                        Add,
                        "  --sys-focus-ring: 0 0 0 4px color-mix(in oklab, var(--sys-color-tint) 60%, transparent);",
                    ),
                ],
            ),
        ],
        "BAU-138" => vec![
            FileDiff::new(
                "controls.rs",
                &[
                    (44, Ctx, "                        <button"),
                    (45, Ctx, "                            type=\"button\""),
                    (
                        46,
                        Del,
                        "                            aria-pressed=move || (value.get() == id).to_string()",
                    ),
                    (46, Add, "                            role=\"radio\""),
                    (
                        47,
                        Add,
                        "                            aria-checked=move || (value.get() == id).to_string()",
                    ),
                    (
                        48,
                        Add,
                        "                            tabindex=move || if value.get() == id { \"0\" } else { \"-1\" }",
                    ),
                    (
                        49,
                        Add,
                        "                            on:keydown=move |ev| select_with_arrows(&ev, options, value)",
                    ),
                    (
                        50,
                        Ctx,
                        "                            on:click=move |_| value.set(id)",
                    ),
                ],
            ),
            FileDiff::new(
                "components.css",
                &[
                    (114, Ctx, ".ui-segmented button:focus-visible {"),
                    (115, Add, "  z-index: 1;"),
                    (116, Ctx, "  box-shadow: var(--sys-focus-ring);"),
                ],
            ),
        ],
        _ => vec![
            FileDiff::new(
                "sheet.rs",
                &[
                    (6, Add, "pub enum Detent {"),
                    (7, Add, "    Medium,"),
                    (8, Add, "    Large,"),
                    (9, Add, "}"),
                    (10, Ctx, ""),
                    (
                        24,
                        Del,
                        "                <div class=\"ui-sheet\" data-detent=\"floating\" role=\"dialog\">",
                    ),
                    (
                        24,
                        Add,
                        "                <div class=\"ui-sheet\" data-detent=move || detent.get().id() role=\"dialog\">",
                    ),
                ],
            ),
            FileDiff::new(
                "components.css",
                &[
                    (
                        220,
                        Ctx,
                        ".ui-sheet[data-detent=\"floating\"] { border-radius: var(--sys-radius-sheet); margin: 0 8px 8px; }",
                    ),
                    (
                        221,
                        Add,
                        ".ui-sheet[data-detent=\"medium\"] { height: 50%; }",
                    ),
                    (
                        222,
                        Add,
                        ".ui-sheet[data-detent=\"large\"] { height: calc(100% - var(--ref-space-48)); }",
                    ),
                ],
            ),
        ],
    }
}
