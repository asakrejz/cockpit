//! „Pull Request anlegen“ for a finished real run: first a preview with editable title and text,
//! then – only on your click – the backend creates the pull request on GitHub (or links the one that
//! already exists for the branch). Nothing happens without the click.

use std::time::Duration;

use cockpit_core::api::PullRequestPreview;
use cockpit_core::{Action, AgentRun, RunKind};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::{route::Route, server, store::use_store};

/// Is this a finished real run whose branch is pushed?
fn eligible(r: &AgentRun) -> bool {
    r.kind == RunKind::Implement && r.real && r.is_done()
}

#[component]
pub fn PullRequestPanel(run: Memo<Option<AgentRun>>) -> impl IntoView {
    let store = use_store();
    let preview = RwSignal::new(None::<PullRequestPreview>);
    let loading = RwSignal::new(false);
    let sending = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);
    let title = RwSignal::new(String::new());
    let body = RwSignal::new(String::new());

    let key = move || run.get().map(|r| r.key()).unwrap_or_default();
    let open_preview = move |_| {
        loading.set(true);
        problem.set(None);
        let key = key();
        spawn_local(async move {
            match server::fetch_pull_request_preview(&key).await {
                Ok(p) => {
                    title.set(p.title.clone());
                    body.set(p.body.clone());
                    preview.set(Some(p));
                }
                Err(e) => problem.set(Some(e.message())),
            }
            loading.set(false);
        });
    };
    let create = move |_| {
        sending.set(true);
        let _ = store.dispatch(Action::CreatePullRequest {
            run: key(),
            title: title.get_untracked(),
            body: body.get_untracked(),
            pull: None,
        });
        set_timeout(move || sending.set(false), Duration::from_secs(4));
    };

    move || {
        let r = run.get().filter(eligible)?;
        // Already there: show the link.
        if let Some(p) = r.pull_request.clone() {
            let number = p.number;
            let has_review = store.ws.with(|w| w.review(number).is_some());
            return Some(
                view! {
                    <div class="ui-list__section" role="region" aria-label="Pull Request">
                        <p class="ui-list__header">"Pull Request"</p>
                        <div class="ui-hstack" style="--spacing: 8px">
                            <Status tone=StatusTone::Green>{format!("Pull Request #{number}")}</Status>
                            <span class="t-footnote t-secondary">
                                {if p.existed { "gab es schon, verlinkt" } else { "angelegt" }}
                            </span>
                        </div>
                        <div class="ui-hstack" style="--spacing: 12px; flex-wrap: wrap">
                            {(!p.url.is_empty())
                                .then(|| {
                                    view! {
                                        <a class="ui-link" href=p.url.clone() target="_blank" rel="noopener noreferrer">
                                            "Auf GitHub öffnen"
                                        </a>
                                    }
                                })}
                            {has_review
                                .then(|| {
                                    view! {
                                        <a class="ui-link" href=Route::Review(Some(number)).hash()>
                                            "Zum Review"
                                        </a>
                                    }
                                })}
                        </div>
                    </div>
                }
                .into_any(),
            );
        }
        let form = preview.get().map(|p| {
            let existing = p.existing.clone();
            let route = format!(
                "Von {} nach {}{} in {}.",
                p.head,
                p.base,
                if p.draft { " (als Entwurf)" } else { "" },
                p.repo
            );
            match existing {
                Some(e) => view! {
                    <div class="ui-vstack" style="--spacing: 10px">
                        <p class="t-subheadline">
                            {format!("Zu diesem Branch gibt es schon den Pull Request #{}. Es wird kein zweiter angelegt – er wird nur verknüpft.", e.number)}
                        </p>
                        <div class="ui-hstack" style="--spacing: 8px; flex-wrap: wrap">
                            <Button
                                variant=ButtonStyle::GlassProminent
                                size=ControlSize::Large
                                disabled=sending
                                on_click=create
                            >
                                {format!("#{} verknüpfen", e.number)}
                            </Button>
                            <Button variant=ButtonStyle::Borderless size=ControlSize::Large on_click=move |_| preview.set(None)>
                                "Abbrechen"
                            </Button>
                        </div>
                    </div>
                }
                .into_any(),
                None => view! {
                    <div class="ui-vstack" style="--spacing: 10px">
                        <p class="t-footnote t-secondary" style="overflow-wrap: anywhere">{route}</p>
                        <TextField label="Titel" value=title />
                        <Textarea value=body label="Beschreibung" min_height="160px" />
                        <p class="t-footnote t-secondary">
                            "Das ist die Vorschau. Erst mit deinem Klick legt das Cockpit den Pull Request bei GitHub an. Gemergt wird nie automatisch."
                        </p>
                        <div class="ui-hstack" style="--spacing: 8px; flex-wrap: wrap">
                            <Button
                                variant=ButtonStyle::GlassProminent
                                size=ControlSize::Large
                                disabled=Signal::derive(move || sending.get() || title.get().trim().is_empty())
                                on_click=create
                            >
                                "Pull Request anlegen"
                            </Button>
                            <Button variant=ButtonStyle::Borderless size=ControlSize::Large on_click=move |_| preview.set(None)>
                                "Abbrechen"
                            </Button>
                        </div>
                    </div>
                }
                .into_any(),
            }
        });
        Some(
            view! {
                <div class="ui-list__section" role="region" aria-label="Pull Request anlegen">
                    <p class="ui-list__header">"Pull Request"</p>
                    <p class="t-footnote t-secondary">
                        "Der Branch ist gepusht. Der Pull Request ist noch nicht angelegt."
                    </p>
                    {form}
                    {move || {
                        preview
                            .get()
                            .is_none()
                            .then(|| {
                                view! {
                                    <div class="ui-hstack">
                                        <Button
                                            variant=ButtonStyle::BorderedProminent
                                            size=ControlSize::Large
                                            disabled=loading
                                            on_click=open_preview
                                        >
                                            {move || if loading.get() { "Lade Vorschau …" } else { "Pull Request anlegen …" }}
                                        </Button>
                                    </div>
                                }
                            })
                    }}
                    {move || {
                        problem
                            .get()
                            .map(|t| {
                                view! {
                                    <StatusBanner
                                        tone=BannerTone::Error
                                        title="Die Vorschau lässt sich nicht laden"
                                        text=t
                                    >
                                        <Button size=ControlSize::Small on_click=open_preview>
                                            "Erneut versuchen"
                                        </Button>
                                    </StatusBanner>
                                }
                            })
                    }}
                </div>
            }
            .into_any(),
        )
    }
}
