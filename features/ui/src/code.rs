//! Diff und Vorschlags-Karte für Code-Reviews.

use leptos::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffKind {
    #[default]
    Context,
    Add,
    Del,
}

/// Eine Zeile im Diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub no: u32,
    pub kind: DiffKind,
    pub text: String,
}

/// Diff einer Datei: Zeilennummer, Vorzeichen, Text. Grün = neu, rot = entfernt.
///
/// Optional lassen sich Zeilen auswählen und Inhalt unter einer Zeile zeigen. Beides
/// kennt nur die Position der Zeile in `rows`, nicht ihre Bedeutung:
///
/// * `on_select`: macht jede Zeile zu einem echten Button (Klick, Enter, Leertaste).
///   Der Callback bekommt die Position der Zeile.
/// * `selected`: Position der hervorgehobenen Zeile.
/// * `below`: liefert Inhalt, der direkt unter einer Zeile steht, z. B. Kommentare.
///   Der Callback läuft einmal je Zeile; Reaktives gehört in eine Closure im Ergebnis.
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

/// Karte für einen Vorschlag. Grün nach Freigabe, abgeblendet nach Ablehnung.
#[component]
pub fn SuggestionCard(
    #[prop(into, default = SuggestionState::Pending.into())] state: Signal<SuggestionState>,
    children: Children,
) -> impl IntoView {
    view! { <div class="ui-suggestion" data-state=move || state.get().id()>{children()}</div> }
}
