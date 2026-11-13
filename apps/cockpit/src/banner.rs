//! Notes about the server and the sources: connecting, not reachable, browser offline, a source
//! with a problem (token rejected, rate limit, outage), or the server has rejected an action.
//! All wording comes from `cockpit_core::notice`.

use cockpit_core::notice::{
    BROWSER_OFFLINE_TEXT, BROWSER_OFFLINE_TITLE, SERVER_UNREACHABLE_TEXT, SERVER_UNREACHABLE_TITLE,
};
use cockpit_core::{NoticeAction, NoticeTone, SourceNotice, source_notices};
use feature_ui::*;
use leptos::prelude::*;

use crate::route::Route;
use crate::store::{Link, Store, use_store};
use crate::sync_status::sync_now;

fn tone(t: NoticeTone) -> BannerTone {
    match t {
        NoticeTone::Info => BannerTone::Info,
        NoticeTone::Warning => BannerTone::Warning,
        NoticeTone::Error => BannerTone::Error,
    }
}

/// One banner for a source, with the action that fits the cause.
fn source_banner(store: Store, n: SourceNotice, busy: RwSignal<bool>) -> impl IntoView {
    let action = match n.action {
        NoticeAction::OpenSetup => view! {
            <Button size=ControlSize::Small variant=ButtonStyle::GlassProminent href=Route::Setup.hash()>
                "Einrichten"
            </Button>
        }
        .into_any(),
        NoticeAction::RetryNow => view! {
            <Button
                size=ControlSize::Small
                disabled=Signal::derive(move || busy.get())
                on_click=move |_| sync_now(store, busy)
            >
                {move || if busy.get() { "Gleiche ab …" } else { "Jetzt abgleichen" }}
            </Button>
        }
        .into_any(),
        NoticeAction::Wait => ().into_any(),
    };
    view! {
        <StatusBanner tone=tone(n.tone) title=n.title text=n.detail>
            {action}
        </StatusBanner>
    }
}

#[component]
pub fn ServerBanner() -> impl IntoView {
    let store = use_store();
    let busy = RwSignal::new(false);

    // The first connection is shown by the gate; this covers everything afterwards.
    let link_banner = move || {
        if !store.loaded.get() {
            return None;
        }
        if !store.browser_online.get() {
            return Some(view! {
                <StatusBanner tone=BannerTone::Warning title=BROWSER_OFFLINE_TITLE text=BROWSER_OFFLINE_TEXT />
            }
            .into_any());
        }
        match store.link.get() {
            Link::Online => None,
            Link::Connecting => Some(view! {
                <StatusBanner title="Verbinde mit dem Server …" />
            }
            .into_any()),
            Link::Offline => Some(view! {
                <StatusBanner
                    tone=BannerTone::Warning
                    title=SERVER_UNREACHABLE_TITLE
                    text=format!("Die Daten unten sind der letzte Stand. {SERVER_UNREACHABLE_TEXT}")
                >
                    <Button
                        size=ControlSize::Small
                        variant=ButtonStyle::GlassProminent
                        on_click=move |_| store.connect()
                    >
                        "Jetzt erneut versuchen"
                    </Button>
                </StatusBanner>
            }
            .into_any()),
        }
    };

    // Sources with a problem; on "Einrichten" the list there already says it.
    let sources = move || {
        if !store.loaded.get() || store.route.get() == Route::Setup {
            return Vec::new();
        }
        let now = store.now.get();
        store.ws.with(|ws| source_notices(&ws.sync, now))
    };

    view! {
        <div class="ck-banner-stack">
            {link_banner}
            <For
                each=sources
                key=|n| (n.source as u8, n.title.clone(), n.detail.clone())
                children=move |n| source_banner(store, n, busy)
            />
            <Show when=move || store.notice.get().is_some()>
                <div class="ck-banner" data-kind="notice" role="alert">
                    <span class="t-subheadline">{move || store.notice.get()}</span>
                    <Button
                        size=ControlSize::Small
                        variant=ButtonStyle::Borderless
                        on_click=move |_| store.notice.set(None)
                    >
                        "OK"
                    </Button>
                </div>
            </Show>
        </div>
    }
}
