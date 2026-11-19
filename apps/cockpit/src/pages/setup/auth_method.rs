//! Picker for the way to sign in ("API-Token" / "OAuth"): one segmented control, the caller shows
//! only the chosen way. Used by the Jira and GitHub steps.

use crate::oauth_card::Service;
use cockpit_core::connections::{AuthKind, ServiceStatus};
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
    /// Called with the way the user just picked (to store the choice).
    #[prop(optional, into)]
    on_pick: Option<Callback<AuthKind>>,
) -> impl IntoView {
    let value = Signal::derive(move || id(chosen.get().unwrap_or_else(|| default.get())));
    let on_change = Callback::new(move |picked: &'static str| {
        let kind = if picked == "oauth" {
            AuthKind::OAuth
        } else {
            AuthKind::Token
        };
        chosen.set(Some(kind));
        if let Some(cb) = on_pick {
            cb.run(kind);
        }
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

/// Stores the picked way as the one the server uses, but only if that way is already set up
/// (otherwise saving its form picks it). `ready` says whether the picked way has credentials.
pub fn persist_pick(ctx: super::fields::Context, service: Service, kind: AuthKind, ready: bool) {
    if !ready {
        return;
    }
    let choice = if kind == AuthKind::OAuth {
        cockpit_core::AuthChoice::OAuth
    } else {
        cockpit_core::AuthChoice::Token
    };
    let mut change = cockpit_core::api::ConfigChange::default();
    match service {
        Service::GitHub => change.github_auth = Some(choice),
        Service::Atlassian => change.jira_auth = Some(choice),
    }
    leptos::task::spawn_local(async move {
        if crate::server::put_config(&change).await.is_ok() {
            ctx.reload();
        }
    });
}

/// The note under the picker when a sign-in is stored that the chosen way does not use.
#[component]
pub fn UnusedSignInNote(#[prop(into)] status: Signal<ServiceStatus>) -> impl IntoView {
    move || {
        let s = status.get();
        let text = match s.choice {
            Some(cockpit_core::AuthChoice::Token) if s.oauth_stored => t!(SetupAuthUnusedOauth),
            _ => return None,
        };
        Some(view! { <p class="t-footnote t-secondary">{text}</p> })
    }
}
