//! Small indicators: status dot, avatar, chip, unread dot.

use leptos::prelude::*;

use crate::{Icon, icons};

/// Color of a status dot. The ID is also the name of the system color
/// (`--sys-color-<id>`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusTone {
    #[default]
    Gray,
    Blue,
    Green,
    Orange,
    Red,
    Indigo,
}

impl StatusTone {
    pub const ALL: [StatusTone; 6] = [
        StatusTone::Gray,
        StatusTone::Blue,
        StatusTone::Green,
        StatusTone::Orange,
        StatusTone::Red,
        StatusTone::Indigo,
    ];

    pub fn id(self) -> &'static str {
        match self {
            StatusTone::Gray => "gray",
            StatusTone::Blue => "blue",
            StatusTone::Green => "green",
            StatusTone::Orange => "orange",
            StatusTone::Red => "red",
            StatusTone::Indigo => "indigo",
        }
    }

    /// `None` = gray is the default, omit the attribute.
    pub fn attr(self) -> Option<&'static str> {
        (self != StatusTone::Gray).then(|| self.id())
    }
}

/// Status with a colored dot, e.g. "Verbunden".
#[component]
pub fn Status(
    #[prop(into, default = StatusTone::Gray.into())] tone: Signal<StatusTone>,
    children: Children,
) -> impl IntoView {
    view! { <span class="ui-status" data-tone=move || tone.get().attr()>{children()}</span> }
}

/// Round avatar with initials. With `agent` it shows the spark in the accent color.
#[component]
pub fn Avatar(
    #[prop(optional, into)] initials: String,
    #[prop(optional)] large: bool,
    #[prop(optional)] agent: bool,
    /// Tooltip, e.g. the full name.
    #[prop(optional, into)]
    title: Option<String>,
) -> impl IntoView {
    view! {
        <span
            class="ui-avatar"
            data-size=large.then_some("large")
            data-kind=agent.then_some("agent")
            title=title
        >
            {if agent {
                view! { <Icon path=icons::SPARKLE size=if large { 20 } else { 15 } /> }.into_any()
            } else {
                initials.into_any()
            }}
        </span>
    }
}

/// Capsule in monospace for branch names and similar short texts.
#[component]
pub fn Chip(children: Children) -> impl IntoView {
    view! { <span class="ui-chip">{children()}</span> }
}

/// Dot for "unread". When read it is invisible but keeps its place.
#[component]
pub fn UnreadDot(#[prop(into)] read: Signal<bool>) -> impl IntoView {
    view! {
        <span class="ui-unread" data-read=move || read.get().then_some("") style="margin-top: 6px">
            <span class="ui-sr-only">{move || if read.get() { String::new() } else { crate::t!(UiUnread) }}</span>
        </span>
    }
}
