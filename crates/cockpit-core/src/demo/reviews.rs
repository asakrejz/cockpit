//! The reviews of the sample data and the sample check runs and diffs.

use super::*;

/// The reviews of #87, #91 (BAU-138) and #85, in this order.
pub(super) fn reviews(pr87: &Item, bau138: &Item, pr85: &Item) -> [Review; 3] {
    let review =
        |number: u32, item: &Item, author: Person, ticket: &str, files: Vec<FileDiff>| Review {
            number,
            repo: item.pr.as_ref().map(|p| p.repo.clone()).unwrap_or_default(),
            title: item.title.clone(),
            description: item.description.clone(),
            description_msg: None,
            author,
            branch: item.branch.clone().unwrap_or_default(),
            base: "main".into(),
            ticket_key: Some(ticket.into()),
            checks: item.pr.as_ref().map_or(Checks::Running, |p| p.checks),
            files,
            agent_reviewed: false,
            agent_summary: String::new(),
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
        agent_summary: String::new(),
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
            pr87,
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
    // Two comments from reviewers on BAU-138: the agent can implement them (follow-up job).
    let foreign =
        |id: &str, who: Person, file: &str, line: usize, text: &str, code: Option<&str>| {
            LineComment {
                id: id.into(),
                file: file.into(),
                line,
                text: text.into(),
                suggestion: code.map(str::to_string),
                author: Some(who),
                resolved: None,
            }
        };
    let review91 = Review {
        line_comments: vec![
            foreign(
                "c1",
                mira(),
                "controls.rs",
                6,
                "Bitte Home und End mit unterstützen, nicht nur die Pfeiltasten.",
                Some(
                    "                            on:keydown=move |ev| select_with_keys(&ev, options, value)",
                ),
            ),
            foreign(
                "c2",
                jonas(),
                "components.css",
                1,
                "Wozu braucht der Fokus-Zustand ein z-index? Bitte kurz begründen oder weglassen.",
                None,
            ),
        ],
        description:
            "Vom Agenten für BAU-138 umgesetzt · Tests bestanden. Der Agent führt keinen Merge aus."
                .into(),
        ..review(
            91,
            bau138,
            Person::agent(),
            "BAU-138",
            agent_changes("BAU-138"),
        )
    };
    let mut review85 = review(
        85,
        pr85,
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
    // Names a second ticket in the body, so that BAU-138 shows two linked pull requests.
    review85
        .description
        .push_str(" Gehört zu BAU-138, siehe auch #91.");
    [review87, review91, review85]
}

/// Three green checks, as a clean pull request has them.
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

/// Checks of #85: the lint error is real, the test is just flaky.
pub(super) fn failing_checks() -> Vec<CheckRun> {
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

/// What the demo agent changes for a ticket.
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
