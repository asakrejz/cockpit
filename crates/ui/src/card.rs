use leptos::prelude::*;

use crate::{Icon, icons};

/// Card / group box. With `href` or `interactive` it reacts to hover and click.
#[component]
pub fn Card(
    /// Lifts on hover (set automatically with `href`).
    #[prop(optional)]
    interactive: bool,
    /// Additional shadow.
    #[prop(optional)]
    elevated: bool,
    #[prop(optional, into)] href: Option<String>,
    children: Children,
) -> impl IntoView {
    let elevated = elevated.then_some("");
    match href {
        Some(href) => view! {
            <a class="ui-card" data-interactive="" data-elevated=elevated href=href>
                {children()}
            </a>
        }
        .into_any(),
        None => view! {
            <div class="ui-card" data-interactive=interactive.then_some("") data-elevated=elevated>
                {children()}
            </div>
        }
        .into_any(),
    }
}

/// Inner padding of a card (below the image).
#[component]
pub fn CardBody(#[prop(optional, into)] css: Option<String>, children: Children) -> impl IntoView {
    view! { <div class="ui-card__body" style=css>{children()}</div> }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaRatio {
    #[default]
    R16x9,
    R4x3,
    R1x1,
    R3x4,
    R21x9,
}

impl MediaRatio {
    pub const ALL: [MediaRatio; 5] = [
        MediaRatio::R16x9,
        MediaRatio::R4x3,
        MediaRatio::R1x1,
        MediaRatio::R3x4,
        MediaRatio::R21x9,
    ];

    pub fn id(self) -> &'static str {
        match self {
            MediaRatio::R16x9 => "16:9",
            MediaRatio::R4x3 => "4:3",
            MediaRatio::R1x1 => "1:1",
            MediaRatio::R3x4 => "3:4",
            MediaRatio::R21x9 => "21:9",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaShape {
    #[default]
    Rectangle,
    /// Circle (always 1:1) – e.g. for avatars.
    Circle,
}

/// Image with a fixed aspect ratio (`Image`).
///
/// Without `src` – or if the image does not load – a placeholder appears.
#[component]
pub fn Media(
    #[prop(optional)] ratio: MediaRatio,
    #[prop(optional)] shape: MediaShape,
    #[prop(optional, into)] src: Option<String>,
    /// Alternative text. Required for real images.
    #[prop(optional, into)]
    alt: String,
    /// Text in the placeholder, e.g. `"[BILD]"`.
    #[prop(default = "[BILD]".into(), into)]
    placeholder: String,
    /// Own width, e.g. `"120px"` (default: full width).
    #[prop(optional, into)]
    width: Option<String>,
) -> impl IntoView {
    let (failed, set_failed) = signal(false);
    let circle = shape == MediaShape::Circle;
    let style = width.map(|w| format!("width:{w};"));

    view! {
        <figure
            class="ui-media"
            data-ratio=(!circle).then_some(ratio.id())
            data-shape=circle.then_some("circle")
            style=style
        >
            {move || match (&src, failed.get()) {
                (Some(src), false) => {
                    view! {
                        <img
                            src=src.clone()
                            alt=alt.clone()
                            loading="lazy"
                            on:error=move |_| set_failed.set(true)
                        />
                    }
                        .into_any()
                }
                _ => {
                    view! {
                        <div class="ui-media__placeholder" role="img" aria-label=alt.clone()>
                            <Icon path=icons::IMAGE size=26 stroke=1.6 />
                            <span>{placeholder.clone()}</span>
                        </div>
                    }
                        .into_any()
                }
            }}
        </figure>
    }
}
