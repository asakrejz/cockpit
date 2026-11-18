//! The naming pattern for the agent's branches of one repo, with a proposal from the repo's
//! guidelines (`CONTRIBUTING.md`). The proposal is only shown: the user confirms it, nothing is
//! taken over by itself.

use cockpit_core::api::BranchSuggestion;
use cockpit_core::branch::{DEFAULT_PATTERN, validate};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::error_text;
use crate::server;

#[component]
pub fn BranchPatternField(
    /// The repo this pattern belongs to (`owner/name`).
    repo: RwSignal<String>,
    pattern: RwSignal<String>,
) -> impl IntoView {
    let loading = RwSignal::new(false);
    let suggestion: RwSignal<Option<BranchSuggestion>> = RwSignal::new(None);
    let problem: RwSignal<Option<String>> = RwSignal::new(None);
    let propose = move |_| {
        let name = repo.get_untracked().trim().to_string();
        loading.set(true);
        problem.set(None);
        suggestion.set(None);
        spawn_local(async move {
            match server::post_branch_suggestion(&name).await {
                Ok(s) => suggestion.set(Some(s)),
                Err(e) => problem.set(Some(error_text(&e))),
            }
            loading.set(false);
        });
    };
    // A pattern that cannot be saved is pointed out right away.
    let invalid = Memo::new(move |_| {
        let p = pattern.get();
        let p = p.trim();
        if p.is_empty() {
            None
        } else {
            validate(p).err()
        }
    });
    view! {
        <TextField
            label="Namensmuster für Branches"
            value=pattern
            placeholder=DEFAULT_PATTERN
            hint="Der Agent benennt seine Branches nach diesem Muster. Platzhalter: {ticket} (Jira-Schlüssel, nötig), {slug} (Kurzname aus dem Titel) und {type} (feature, fix oder chore). Leer lassen für die Voreinstellung."
            autocomplete="off"
        />
        {move || invalid.get().map(|m| view! { <p class="ui-hint" role="alert" style="color: var(--sys-color-red)">{m}</p> })}
        <HStack spacing="12px">
            <Button
                variant=ButtonStyle::Borderless
                on_click=propose
                disabled=Signal::derive(move || loading.get())
            >
                {move || if loading.get() { "Lese Richtlinien …" } else { "Aus Richtlinien vorschlagen" }}
            </Button>
        </HStack>
        {move || problem.get().map(|m| view! { <p class="ui-hint" role="alert" style="color: var(--sys-color-red)">{m}</p> })}
        {move || {
            suggestion
                .get()
                .map(|s| {
                    let proposed = s.pattern.clone();
                    view! {
                        <div role="status">
                            <p class="ui-hint">
                                {s.source.as_ref().map(|f| format!("Aus {f}: ")).unwrap_or_default()}
                                {s.message.clone()}
                            </p>
                            {proposed
                                .map(|p| {
                                    let take = p.clone();
                                    view! {
                                        <p class="ui-hint">
                                            "Vorschlag: " <code>{p}</code>
                                        </p>
                                        <HStack spacing="12px">
                                            <Button on_click=move |_| {
                                                pattern.set(take.clone());
                                                suggestion.set(None);
                                            }>"Übernehmen"</Button>
                                            <Button
                                                variant=ButtonStyle::Borderless
                                                on_click=move |_| suggestion.set(None)
                                            >
                                                "Verwerfen"
                                            </Button>
                                        </HStack>
                                    }
                                })}
                        </div>
                    }
                })
        }}
    }
}
