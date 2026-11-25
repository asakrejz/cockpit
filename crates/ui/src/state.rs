//! States instead of blank areas: empty, loading, error and a status banner.
//!
//! * [`EmptyState`]: nothing here yet – says why and offers the next step (children).
//! * [`ErrorState`]: something failed – says the cause and what to do (children); announced
//!   as an alert.
//! * [`StatusBanner`]: a slim bar for a state of the whole page or one data source.
//! * [`Skeleton`]: placeholder rows while loading; keeps the height, so nothing jumps.

use leptos::prelude::*;

use crate::icon::Icon;

/// Importance of a [`StatusBanner`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BannerTone {
    /// Informational: nothing is lost (a pause, a retry in progress).
    #[default]
    Info,
    /// Something is wrong, but the last data stays visible.
    Warning,
    /// Needs the user.
    Error,
}

impl BannerTone {
    pub fn id(self) -> &'static str {
        match self {
            BannerTone::Info => "info",
            BannerTone::Warning => "warning",
            BannerTone::Error => "error",
        }
    }
}

/// Nothing here yet: a title, one sentence of explanation and (as children) the next action.
#[component]
pub fn EmptyState(
    #[prop(into)] title: Signal<String>,
    #[prop(optional, into)] text: MaybeProp<String>,
    /// Icon path from `icons`.
    #[prop(optional)]
    icon: Option<&'static str>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="ui-state" data-kind="empty">
            {icon
                .map(|path| {
                    view! {
                        <span class="ui-state__icon" aria-hidden="true">
                            <Icon path=path size=22 />
                        </span>
                    }
                })}
            <p class="t-headline ui-state__title">{move || title.get()}</p>
            {move || text.get().map(|t| view! { <p class="t-subheadline t-secondary ui-state__text">{t}</p> })}
            {children.map(|c| view! { <div class="ui-state__actions">{c()}</div> })}
        </div>
    }
}

/// Something failed: cause in plain words, then what to do (children). Read out as an alert.
#[component]
pub fn ErrorState(
    #[prop(into)] title: Signal<String>,
    #[prop(optional, into)] text: MaybeProp<String>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="ui-state" data-kind="error" role="alert">
            <span class="ui-state__icon" aria-hidden="true">
                <Icon path=crate::icon::icons::ALERT size=22 />
            </span>
            <p class="t-headline ui-state__title">{move || title.get()}</p>
            {move || text.get().map(|t| view! { <p class="t-subheadline t-secondary ui-state__text">{t}</p> })}
            {children.map(|c| view! { <div class="ui-state__actions">{c()}</div> })}
        </div>
    }
}

/// A slim bar: title, optional detail and actions (children). `Error` is announced at once
/// (`role="alert"`), the rest politely (`role="status"`).
#[component]
pub fn StatusBanner(
    #[prop(optional)] tone: BannerTone,
    #[prop(into)] title: Signal<String>,
    #[prop(optional, into)] text: MaybeProp<String>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    let role = if tone == BannerTone::Error {
        "alert"
    } else {
        "status"
    };
    view! {
        <div class="ui-banner" data-tone=tone.id() role=role>
            <div class="ui-banner__text">
                <span class="t-subheadline t-semibold">{move || title.get()}</span>
                {move || text.get().map(|t| view! { <span class="t-footnote t-secondary">{t}</span> })}
            </div>
            {children.map(|c| view! { <div class="ui-banner__actions">{c()}</div> })}
        </div>
    }
}

/// Placeholder rows while data loads. `label` is read out once ("Loading …").
#[component]
pub fn Skeleton(
    #[prop(default = 3)] rows: usize,
    #[prop(into, default = "Lädt …".to_string())] label: String,
) -> impl IntoView {
    view! {
        <div class="ui-skeleton" role="status" aria-busy="true">
            <span class="ui-sr-only">{label}</span>
            {(0..rows)
                .map(|i| {
                    view! { <span class="ui-skeleton__row" data-short=(i % 3 == 2).then_some("") aria-hidden="true"></span> }
                })
                .collect_view()}
        </div>
    }
}
