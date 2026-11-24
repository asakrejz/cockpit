use super::*;

use cockpit_core::DiffSide;

/// Where a suggestion sits, in words: `datei · Zeile 4`, `datei · Zeilen 2–4` or, on the removed
/// side, `datei · Zeile 4 (alte Zeile)`. Without a structured position the stored text.
pub(super) fn location_label(s: &Suggestion) -> String {
    if s.path.is_empty() || s.line == 0 {
        return s.location.clone();
    }
    let lines = match s.start_line {
        Some(start) => t!(ReviewDraftsLines, start = start, end = s.line),
        None => t!(ReviewLine, n = s.line),
    };
    if s.side == DiffSide::Left {
        t!(ReviewDraftsWhereOld, path = s.path, lines = lines)
    } else {
        t!(ReviewDraftsWhere, path = s.path, lines = lines)
    }
}

/// The replacement code of a draft as a small diff: the lines it replaces (red) and the new ones
/// (green). Empty if the draft has no code.
pub(super) fn code_rows(r: &Review, s: &Suggestion) -> Vec<DiffRow> {
    let Some(code) = s.code.as_deref() else {
        return Vec::new();
    };
    let first = s.start_line.unwrap_or(s.line);
    let mut rows: Vec<DiffRow> = r
        .files
        .iter()
        .find(|f| f.name == s.path)
        .map(|f| {
            f.lines
                .iter()
                .filter(|l| l.no >= first && l.no <= s.line && DiffSide::Right.has(l.kind))
                .map(|l| DiffRow {
                    no: l.no,
                    kind: DiffKind::Del,
                    text: l.text.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    rows.extend(code.lines().enumerate().map(|(i, text)| DiffRow {
        no: first + i as u32,
        kind: DiffKind::Add,
        text: text.to_string(),
    }));
    rows
}

/// Drafts of the agent under a diff line: text, code and the decision. Editing is in the list on
/// the right; here you only approve or discard.
pub(super) fn draft_notes(store: Store, r: &Review, file: &str, index: usize) -> AnyView {
    let number = StoredValue::new(r.key());
    r.suggestions_at(file, index)
        .into_iter()
        .map(|s| {
            let id = StoredValue::new(s.id.clone());
            let decide = move |decision: Option<Decision>| {
                move |_| {
                    let _ = store.dispatch(Action::Decide {
                        review: number.get_value(),
                        suggestion: id.get_value(),
                        decision,
                    });
                }
            };
            let rows = code_rows(r, s);
            let controls = match s.decision {
                None => view! {
                    <div class="ui-hstack" style="--spacing: 8px">
                        <Button
                            variant=ButtonStyle::BorderedProminent
                            size=ControlSize::Small
                            on_click=decide(Some(Decision::Approved))
                        >
                            {t!(ReviewApprove)}
                        </Button>
                        <Button size=ControlSize::Small destructive=true on_click=decide(Some(Decision::Declined))>
                            {t!(ReviewDiscard)}
                        </Button>
                    </div>
                }
                .into_any(),
                Some(d) => {
                    let (label, tone) = match d {
                        Decision::Approved => (t!(ReviewDraftsApproved), StatusTone::Green),
                        Decision::Declined => (t!(ReviewDraftsDeclined), StatusTone::Gray),
                    };
                    view! {
                        <div class="ui-hstack" style="justify-content: space-between">
                            <Status tone=tone>{label}</Status>
                            <Button variant=ButtonStyle::Borderless size=ControlSize::Small on_click=decide(None)>
                                {t!(ReviewUndo)}
                            </Button>
                        </div>
                    }
                    .into_any()
                }
            };
            view! {
                <div class="ui-diff__note" role="group" aria-label=t!(ReviewDraftsGroup)>
                    <span class="ui-hstack t-footnote t-secondary" style="--spacing: 8px">
                        {t!(ReviewDraftsNote)}
                    </span>
                    <Markdown text=s.text.clone() class="t-subheadline" />
                    {(!rows.is_empty()).then(|| view! { <Diff rows=rows.clone() label=t!(ReviewSuggestion) /> })}
                    {controls}
                </div>
            }
        })
        .collect_view()
        .into_any()
}
