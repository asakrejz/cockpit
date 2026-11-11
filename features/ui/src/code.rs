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
#[component]
pub fn Diff(rows: Vec<DiffRow>, #[prop(into)] label: String) -> impl IntoView {
    view! {
        <div class="ui-diff" role="group" aria-label=label>
            {rows
                .into_iter()
                .map(|row| {
                    let (kind, sign) = match row.kind {
                        DiffKind::Context => ("ctx", " "),
                        DiffKind::Add => ("add", "+"),
                        DiffKind::Del => ("del", "−"),
                    };
                    view! {
                        <div class="ui-diff__line" data-kind=kind>
                            <span class="ui-diff__no">{row.no}</span>
                            <span>{sign}</span>
                            <span>{row.text}</span>
                        </div>
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
