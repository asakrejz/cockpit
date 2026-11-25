use leptos::prelude::*;

/// Text Styles (Dynamic Type "Large") – classes `t-*`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextStyle {
    Hero,
    Display,
    LargeTitle,
    Title1,
    Title2,
    Title3,
    Headline,
    #[default]
    Body,
    Callout,
    Subheadline,
    Footnote,
    Caption1,
    Caption2,
}

impl TextStyle {
    pub const ALL: [TextStyle; 13] = [
        TextStyle::Hero,
        TextStyle::Display,
        TextStyle::LargeTitle,
        TextStyle::Title1,
        TextStyle::Title2,
        TextStyle::Title3,
        TextStyle::Headline,
        TextStyle::Body,
        TextStyle::Callout,
        TextStyle::Subheadline,
        TextStyle::Footnote,
        TextStyle::Caption1,
        TextStyle::Caption2,
    ];

    /// Name in the CSS (`t-<id>`) and in the tokens (`--sys-type-<id>`).
    pub fn id(self) -> &'static str {
        match self {
            TextStyle::Hero => "hero",
            TextStyle::Display => "display",
            TextStyle::LargeTitle => "large-title",
            TextStyle::Title1 => "title1",
            TextStyle::Title2 => "title2",
            TextStyle::Title3 => "title3",
            TextStyle::Headline => "headline",
            TextStyle::Body => "body",
            TextStyle::Callout => "callout",
            TextStyle::Subheadline => "subheadline",
            TextStyle::Footnote => "footnote",
            TextStyle::Caption1 => "caption1",
            TextStyle::Caption2 => "caption2",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextColor {
    /// `label`
    #[default]
    Primary,
    /// `secondaryLabel`
    Secondary,
    /// `tint`
    Tint,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextTag {
    #[default]
    P,
    Span,
    H1,
    H2,
    H3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWeight {
    #[default]
    Regular,
    Semibold,
    Bold,
}

/// Text with a text style from the design.
///
/// ```ignore
/// view! { <Text variant=TextStyle::Title1 weight=TextWeight::Bold tag=TextTag::H2>"Title"</Text> }
/// ```
#[component]
pub fn Text(
    #[prop(optional)] variant: TextStyle,
    #[prop(optional)] color: TextColor,
    #[prop(optional)] weight: TextWeight,
    #[prop(optional)] tag: TextTag,
    /// Monospace font (for token names and code).
    #[prop(optional)]
    mono: bool,
    children: Children,
) -> impl IntoView {
    let mut class = format!("t-{}", variant.id());
    match weight {
        TextWeight::Regular => {}
        TextWeight::Semibold => class.push_str(" t-semibold"),
        TextWeight::Bold => class.push_str(" t-bold"),
    }
    match color {
        TextColor::Primary => {}
        TextColor::Secondary => class.push_str(" t-secondary"),
        TextColor::Tint => class.push_str(" t-tint"),
    }
    if mono {
        class.push_str(" t-mono");
    }
    let children = children();
    match tag {
        TextTag::P => view! { <p class=class>{children}</p> }.into_any(),
        TextTag::Span => view! { <span class=class>{children}</span> }.into_any(),
        TextTag::H1 => view! { <h1 class=class>{children}</h1> }.into_any(),
        TextTag::H2 => view! { <h2 class=class>{children}</h2> }.into_any(),
        TextTag::H3 => view! { <h3 class=class>{children}</h3> }.into_any(),
    }
}
