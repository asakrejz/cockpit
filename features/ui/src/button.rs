use leptos::{ev, prelude::*};

use crate::Size;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Outline,
    Ghost,
    Danger,
}

impl ButtonVariant {
    pub const ALL: [ButtonVariant; 5] = [
        ButtonVariant::Primary,
        ButtonVariant::Secondary,
        ButtonVariant::Outline,
        ButtonVariant::Ghost,
        ButtonVariant::Danger,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ButtonVariant::Primary => "primary",
            ButtonVariant::Secondary => "secondary",
            ButtonVariant::Outline => "outline",
            ButtonVariant::Ghost => "ghost",
            ButtonVariant::Danger => "danger",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name() == name)
    }
}

/// Ein Button mit Varianten, Größen und Zuständen.
///
/// `variant`, `size`, `disabled` und `loading` akzeptieren sowohl feste Werte
/// als auch Signals – so lassen sie sich zur Laufzeit ändern.
///
/// ```ignore
/// view! { <Button variant=ButtonVariant::Danger size=Size::Lg on_click=...>"Löschen"</Button> }
/// ```
#[component]
pub fn Button(
    #[prop(into, default = ButtonVariant::Primary.into())] variant: Signal<ButtonVariant>,
    #[prop(into, default = Size::Md.into())] size: Signal<Size>,
    /// Deaktiviert den Button.
    #[prop(into, default = false.into())]
    disabled: Signal<bool>,
    /// Zeigt einen Spinner und blockiert Klicks.
    #[prop(into, default = false.into())]
    loading: Signal<bool>,
    /// Button füllt die volle Breite.
    #[prop(optional)]
    full_width: bool,
    #[prop(optional, into)] on_click: Option<Callback<ev::MouseEvent>>,
    children: Children,
) -> impl IntoView {
    let class = move || {
        let mut class = format!(
            "ui-button ui-button--{} ui-button--{}",
            variant.get().name(),
            size.get().name()
        );
        if full_width {
            class.push_str(" ui-button--block");
        }
        if loading.get() {
            class.push_str(" ui-button--loading");
        }
        class
    };
    let is_disabled = move || disabled.get() || loading.get();

    view! {
        <button
            type="button"
            class=class
            disabled=is_disabled
            aria-busy=move || loading.get().to_string()
            on:click=move |ev| {
                if let Some(cb) = on_click {
                    cb.run(ev);
                }
            }
        >
            <span class="ui-button__spinner" aria-hidden="true" hidden=move || !loading.get()></span>
            {children()}
        </button>
    }
}
