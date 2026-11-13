//! Diff and suggestion card for code reviews.

use leptos::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffKind {
    #[default]
    Context,
    Add,
    Del,
}

/// A line in the diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub no: u32,
    pub kind: DiffKind,
    pub text: String,
}

/// Diff of a file: line number, sign, text. Green = new, red = removed.
///
/// Optionally lines can be selected and content shown below a line. Both
/// only know the position of the line in `rows`, not its meaning:
///
/// * `on_select`: makes every line a real button (click, Enter, space).
///   The callback gets the position of the line.
/// * `selected`: position of the highlighted line.
/// * `below`: delivers content that stands directly below a line, e.g. comments.
///   The callback runs once per line; reactive things belong in a closure in the result.
#[component]
pub fn Diff(
    rows: Vec<DiffRow>,
    #[prop(into)] label: String,
    #[prop(default = None)] on_select: Option<Callback<usize>>,
    #[prop(into, default = None.into())] selected: Signal<Option<usize>>,
    #[prop(default = None)] below: Option<Callback<usize, AnyView>>,
) -> impl IntoView {
    view! {
        <div class="ui-diff" role="group" aria-label=label>
            {rows
                .into_iter()
                .enumerate()
                .map(|(index, row)| {
                    let (kind, sign) = match row.kind {
                        DiffKind::Context => ("ctx", " "),
                        DiffKind::Add => ("add", "+"),
                        DiffKind::Del => ("del", "−"),
                    };
                    let no = row.no;
                    let cells = view! {
                        <span class="ui-diff__no">{no}</span>
                        <span>{sign}</span>
                        <span>{row.text}</span>
                    };
                    let line = match on_select {
                        Some(on_select) => {
                            let description = match row.kind {
                                DiffKind::Context => "",
                                DiffKind::Add => ", neu",
                                DiffKind::Del => ", entfernt",
                            };
                            view! {
                                <button
                                    type="button"
                                    class="ui-diff__line ui-diff__line--select"
                                    data-kind=kind
                                    aria-label=format!("Zeile {no}{description} auswählen")
                                    aria-pressed=move || {
                                        (selected.get() == Some(index)).to_string()
                                    }
                                    on:click=move |_| on_select.run(index)
                                >
                                    {cells}
                                </button>
                            }
                                .into_any()
                        }
                        None => {
                            view! {
                                <div class="ui-diff__line" data-kind=kind>
                                    {cells}
                                </div>
                            }
                                .into_any()
                        }
                    };
                    let below = below.map(|below| view! { <div class="ui-diff__below">{below.run(index)}</div> });
                    view! {
                        {line}
                        {below}
                    }
                })
                .collect_view()}
        </div>
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SuggestionState {
    #[default]
    Pending,
    Approved,
    Declined,
}

impl SuggestionState {
    pub fn id(self) -> &'static str {
        match self {
            SuggestionState::Pending => "pending",
            SuggestionState::Approved => "approved",
            SuggestionState::Declined => "declined",
        }
    }
}

/// Card for a suggestion. Green after approval, dimmed after rejection.
#[component]
pub fn SuggestionCard(
    #[prop(into, default = SuggestionState::Pending.into())] state: Signal<SuggestionState>,
    children: Children,
) -> impl IntoView {
    view! { <div class="ui-suggestion" data-state=move || state.get().id()>{children()}</div> }
}
