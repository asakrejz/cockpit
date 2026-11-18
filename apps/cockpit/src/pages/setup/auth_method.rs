//! Picker for the way to sign in ("API-Token" / "OAuth"): one segmented control, the caller shows
//! only the chosen way. Used by the Jira step; the GitHub step can use it the same way.

use cockpit_core::connections::AuthKind;
use feature_ui::*;
use leptos::prelude::*;

/// The two ways: (id, label). The labels follow the language, so they are built on demand.
fn options() -> [(&'static str, String); 2] {
    [("token", t!(SetupAuthToken)), ("oauth", t!(SetupAuthOauth))]
}

fn id(kind: AuthKind) -> &'static str {
    if kind == AuthKind::OAuth {
        "oauth"
    } else {
        "token"
    }
}

/// The picker. `chosen` is what the user picked (`None`: nothing yet); `default` is shown until
/// then (e.g. the connected way, see `cockpit_core::setup::jira_default_method`).
#[component]
pub fn AuthMethodPicker(
    chosen: RwSignal<Option<AuthKind>>,
    #[prop(into)] default: Signal<AuthKind>,
    #[prop(into)] aria_label: String,
) -> impl IntoView {
    let value = Signal::derive(move || id(chosen.get().unwrap_or_else(|| default.get())));
    let on_change = Callback::new(move |picked: &'static str| {
        chosen.set(Some(if picked == "oauth" {
            AuthKind::OAuth
        } else {
            AuthKind::Token
        }));
    });
    view! {
        <div class="ui-segmented" role="group" aria-label=aria_label style="align-self: stretch">
            {options()
                .into_iter()
                .map(|(id, label)| {
                    view! {
                        <button
                            type="button"
                            style="flex: 1 1 0; padding-inline: 8px"
                            aria-pressed=move || (value.get() == id).to_string()
                            on:click=move |_| on_change.run(id)
                        >
                            {label}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// The way that is shown now.
pub fn current(chosen: RwSignal<Option<AuthKind>>, default: Signal<AuthKind>) -> Memo<AuthKind> {
    Memo::new(move |_| chosen.get().unwrap_or_else(|| default.get()))
}
