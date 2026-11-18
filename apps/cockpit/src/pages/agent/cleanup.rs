//! „Aufräumen“: first a preview of what no working run needs any more, then – only after the
//! click on „Jetzt entfernen“ – the removal of exactly what the preview showed.

use cockpit_core::api::{CleanupPreview, CleanupRequest};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::texts as t;
use crate::server;

#[component]
pub(super) fn CleanupButton() -> impl IntoView {
    let open = RwSignal::new(false);
    let preview = RwSignal::new(None::<CleanupPreview>);
    let loading = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);
    let result = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    let show = move |_| {
        preview.set(None);
        result.set(None);
        problem.set(None);
        loading.set(true);
        open.set(true);
        spawn_local(async move {
            match server::fetch_cleanup_preview().await {
                Ok(p) => preview.set(Some(p)),
                Err(e) => problem.set(Some(e.message())),
            }
            loading.set(false);
        });
    };
    let remove = move |_| {
        let Some(p) = preview.get_untracked() else {
            return;
        };
        busy.set(true);
        let request = CleanupRequest {
            containers: p.containers.iter().map(|c| c.id.clone()).collect(),
            folders: p.folders.clone(),
        };
        spawn_local(async move {
            match server::post_cleanup(&request).await {
                Ok(r) => {
                    result.set(Some(t::cleanup_done(r.containers, r.folders)));
                    preview.set(None);
                }
                Err(e) => problem.set(Some(e.message())),
            }
            busy.set(false);
        });
    };
    let nothing_to_remove = Signal::derive(move || {
        busy.get() || preview.with(|p| p.as_ref().is_none_or(CleanupPreview::is_empty))
    });

    let body = move || {
        view! {
            <div class="ck-dialog">
                {move || loading.get().then(|| view! { <p class="ui-hint" role="status">{t::cleanup_loading()}</p> })}
                {move || problem.get().map(|m| view! { <p class="ui-hint ck-problem" role="alert">{m}</p> })}
                {move || result.get().map(|m| view! { <p class="t-subheadline" role="status">{m}</p> })}
                {move || {
                    preview
                        .get()
                        .map(|p| {
                            if p.is_empty() && !p.engine_missing {
                                return view! { <p class="t-subheadline">{t::cleanup_nothing()}</p> }.into_any();
                            }
                            view! {
                                <p class="t-footnote t-secondary">{t::cleanup_intro()}</p>
                                {p
                                    .engine_missing
                                    .then(|| view! { <p class="ui-hint">{t::cleanup_no_engine()}</p> })}
                                {(!p.containers.is_empty())
                                    .then(|| {
                                        view! {
                                            <h2 class="t-headline ck-dialog__heading">{t::cleanup_containers()}</h2>
                                            <ul class="ck-cleanup__list">
                                                {p
                                                    .containers
                                                    .iter()
                                                    .map(|c| view! { <li class="t-subheadline t-mono">{c.name.clone()}</li> })
                                                    .collect_view()}
                                            </ul>
                                        }
                                    })}
                                {(!p.folders.is_empty())
                                    .then(|| {
                                        view! {
                                            <h2 class="t-headline ck-dialog__heading">{t::cleanup_folders()}</h2>
                                            <ul class="ck-cleanup__list">
                                                {p
                                                    .folders
                                                    .iter()
                                                    .map(|f| view! { <li class="t-subheadline t-mono">{f.clone()}</li> })
                                                    .collect_view()}
                                            </ul>
                                        }
                                    })}
                            }
                                .into_any()
                        })
                }}
            </div>
        }
    };
    let footer = move || {
        view! {
            <Button on_click=move |_| open.set(false)>{t::close()}</Button>
            <Button variant=ButtonStyle::GlassProminent disabled=nothing_to_remove on_click=remove>
                {t::cleanup_run()}
            </Button>
        }
    };
    view! {
        <Button on_click=show>{t::cleanup_button()}</Button>
        <Dialog open=open title=t::cleanup_title() footer=footer>
            {body}
        </Dialog>
    }
}
