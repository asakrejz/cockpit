//! State of the sync with Jira and GitHub per source, plus "Sync now".
//! The data is in the workspace (`sync`), the sync runs in the
//! backend. No key is here.

use cockpit_core::SourceState;
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server;
use crate::store::{Store, now_ms, use_store};

fn subtitle(state: &SourceState, now: u64) -> String {
    let last = state
        .last_ok_ms
        .map_or(t!(SyncNever), |t| t!(SyncLast, ago = ago(t, now)));
    match &state.problem {
        None => {
            // The structured hint reads in the current language; old data only has the text.
            let warning = state
                .warning_msgs
                .first()
                .map(|m| m.render(lang()))
                .or_else(|| state.warnings.first().cloned());
            match warning {
                Some(w) => t!(SyncLastWarning, last = last, warning = w),
                None => last,
            }
        }
        Some(p) => t!(
            SyncLastProblem,
            problem = p.msg().render(lang()),
            last = last
        ),
    }
}

/// "Sync now": asks the server to sync now and reports the outcome as a notice.
pub fn sync_now(store: Store, busy: RwSignal<bool>) {
    busy.set(true);
    spawn_local(async move {
        let result = server::post_sync().await;
        busy.set(false);
        match result {
            Ok(r) if !r.active => store.notify(t!(SyncNoSource)),
            Ok(_) => {} // The state comes through the event stream.
            Err(e) => store.notify(e.message()),
        }
    });
}

#[component]
pub fn SyncStatus() -> impl IntoView {
    let store = use_store();
    let busy = RwSignal::new(false);

    let sync_now = move |_| sync_now(store, busy);

    let rows = move || {
        let now = now_ms();
        let states = store.ws.with(|ws| ws.sync.sources.clone());
        if states.is_empty() {
            return view! {
                <ListRow
                    title=t!(SyncEmptyTitle)
                    subtitle=t!(SyncEmptyText)
                    wrap=true
                />
            }
            .into_any();
        }
        states
            .into_iter()
            .map(|st| {
                let name = st.source.label();
                let ok = st.is_ok();
                view! {
                    <ListRow title=name subtitle=subtitle(&st, now) wrap=true>
                        <Badge variant=if ok { BadgeVariant::Tint } else { BadgeVariant::Neutral }>
                            {if ok { t!(SyncConnected) } else { t!(SyncProblem) }}
                        </Badge>
                    </ListRow>
                }
            })
            .collect_view()
            .into_any()
    };

    view! {
        <List>
                <ListSection
                    header=t!(SyncHeader)
                    footer=t!(SyncFooter)
                >
                    {rows}
                    <ListRow title=t!(BannerSyncNow) subtitle=t!(SyncNowHint)>
                        <Button disabled=Signal::derive(move || busy.get()) on_click=sync_now>
                            {move || if busy.get() { t!(BannerSyncing) } else { t!(BannerSyncNow) }}
                        </Button>
                    </ListRow>
                </ListSection>
        </List>
    }
}
