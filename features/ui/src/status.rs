//! Kleine Anzeigen: Status-Punkt, Avatar, Chip, Ungelesen-Punkt.

use leptos::prelude::*;

use crate::{Icon, icons};

/// Farbe eines Status-Punkts. Die ID ist zugleich der Name der System-Farbe
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

    /// `None` = Grau ist der Standard, Attribut weglassen.
    pub fn attr(self) -> Option<&'static str> {
        (self != StatusTone::Gray).then(|| self.id())
    }
}

/// Status mit farbigem Punkt, z. B. „Verbunden“.
#[component]
pub fn Status(
    #[prop(into, default = StatusTone::Gray.into())] tone: Signal<StatusTone>,
    children: Children,
) -> impl IntoView {
    view! { <span class="ui-status" data-tone=move || tone.get().attr()>{children()}</span> }
}

/// Runder Avatar mit Initialen. Mit `agent` zeigt er den Funken in der Akzentfarbe.
#[component]
pub fn Avatar(
    #[prop(optional, into)] initials: String,
    #[prop(optional)] large: bool,
    #[prop(optional)] agent: bool,
    /// Tooltip, z. B. der volle Name.
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

/// Kapsel in Festbreitenschrift für Branch-Namen und ähnliche Kurztexte.
#[component]
pub fn Chip(children: Children) -> impl IntoView {
    view! { <span class="ui-chip">{children()}</span> }
}

/// Punkt für „ungelesen“. Gelesen ist er unsichtbar, behält aber seinen Platz.
#[component]
pub fn UnreadDot(#[prop(into)] read: Signal<bool>) -> impl IntoView {
    view! {
        <span class="ui-unread" data-read=move || read.get().then_some("") style="margin-top: 6px">
            <span class="ui-sr-only">{move || if read.get() { "" } else { "Ungelesen" }}</span>
        </span>
    }
}
