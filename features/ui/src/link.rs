use leptos::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkVariant {
    /// Akzentfarbe.
    #[default]
    Default,
    /// Dezent, in Textfarbe.
    Muted,
    /// Sieht aus wie ein Button (für Call-to-Actions).
    Button,
}

impl LinkVariant {
    pub const ALL: [LinkVariant; 3] = [
        LinkVariant::Default,
        LinkVariant::Muted,
        LinkVariant::Button,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LinkVariant::Default => "default",
            LinkVariant::Muted => "muted",
            LinkVariant::Button => "button",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkUnderline {
    Always,
    #[default]
    Hover,
    Never,
}

impl LinkUnderline {
    pub const ALL: [LinkUnderline; 3] = [
        LinkUnderline::Always,
        LinkUnderline::Hover,
        LinkUnderline::Never,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LinkUnderline::Always => "always",
            LinkUnderline::Hover => "hover",
            LinkUnderline::Never => "never",
        }
    }
}

/// Ein Link. Mit `external` öffnet er in einem neuen Tab – inklusive
/// `rel="noopener noreferrer"` (Sicherheit) und einem ↗-Symbol.
#[component]
pub fn Link(
    #[prop(into)] href: String,
    #[prop(default = LinkVariant::Default)] variant: LinkVariant,
    #[prop(default = LinkUnderline::Hover)] underline: LinkUnderline,
    #[prop(optional)] external: bool,
    #[prop(optional)] disabled: bool,
    children: Children,
) -> impl IntoView {
    let class = format!(
        "ui-link ui-link--{} ui-link--underline-{}{}",
        variant.name(),
        underline.name(),
        if disabled { " ui-link--disabled" } else { "" }
    );

    view! {
        <a
            class=class
            // Ein deaktivierter Link bekommt kein href und wird dadurch nicht anklickbar.
            href=(!disabled).then_some(href)
            target=external.then_some("_blank")
            rel=external.then_some("noopener noreferrer")
            aria-disabled=disabled.then_some("true")
        >
            {children()}
            {external.then(|| view! { <span class="ui-link__icon" aria-hidden="true">" ↗"</span> })}
        </a>
    }
}
