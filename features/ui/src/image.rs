use leptos::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// Füllt den Rahmen, Ränder werden ggf. abgeschnitten.
    #[default]
    Cover,
    /// Zeigt das ganze Bild, ggf. mit Leerraum.
    Contain,
    /// Streckt das Bild auf die Rahmengröße.
    Fill,
}

impl ImageFit {
    pub const ALL: [ImageFit; 3] = [ImageFit::Cover, ImageFit::Contain, ImageFit::Fill];

    pub fn name(self) -> &'static str {
        match self {
            ImageFit::Cover => "cover",
            ImageFit::Contain => "contain",
            ImageFit::Fill => "fill",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageRadius {
    #[default]
    None,
    Md,
    /// Kreis (bei quadratischem Rahmen) – z. B. für Avatare.
    Full,
}

impl ImageRadius {
    pub const ALL: [ImageRadius; 3] = [ImageRadius::None, ImageRadius::Md, ImageRadius::Full];

    pub fn name(self) -> &'static str {
        match self {
            ImageRadius::None => "none",
            ImageRadius::Md => "md",
            ImageRadius::Full => "full",
        }
    }
}

/// Ein Bild mit Seitenverhältnis, Zuschnitt, Rundung, Lazy-Loading,
/// Fallback bei Ladefehler und optionaler Bildunterschrift.
#[component]
pub fn Image(
    #[prop(into)] src: String,
    /// Alternativtext – Pflicht, damit Barrierefreiheit nicht vergessen wird.
    #[prop(into)]
    alt: String,
    /// Breite in px.
    #[prop(optional)]
    width: Option<u32>,
    /// Seitenverhältnis als CSS-Wert, z. B. `"16 / 9"` oder `"1"`.
    #[prop(optional, into)]
    ratio: Option<String>,
    #[prop(default = ImageFit::Cover)] fit: ImageFit,
    #[prop(default = ImageRadius::None)] radius: ImageRadius,
    /// `true` = erst laden, wenn das Bild in die Nähe des Sichtbereichs kommt.
    #[prop(default = true)]
    lazy: bool,
    #[prop(optional, into)] caption: Option<String>,
) -> impl IntoView {
    let (failed, set_failed) = signal(false);

    let class = format!(
        "ui-image ui-image--fit-{} ui-image--radius-{}",
        fit.name(),
        radius.name()
    );
    let style = {
        let mut style = String::new();
        if let Some(w) = width {
            style.push_str(&format!("width:{w}px;"));
        }
        if let Some(r) = &ratio {
            style.push_str(&format!("aspect-ratio:{r};"));
        }
        style
    };
    let alt_fallback = alt.clone();

    view! {
        <figure class="ui-figure" style=width.map(|w| format!("width:{w}px;"))>
            <div class=class style=style>
                <Show
                    when=move || !failed.get()
                    fallback=move || {
                        view! {
                            <div class="ui-image__fallback" role="img" aria-label=alt_fallback.clone()>
                                "🖼️ Bild nicht verfügbar"
                            </div>
                        }
                    }
                >
                    <img
                        src=src.clone()
                        alt=alt.clone()
                        loading=if lazy { "lazy" } else { "eager" }
                        on:error=move |_| set_failed.set(true)
                    />
                </Show>
            </div>
            {caption.map(|c| view! { <figcaption class="ui-image__caption">{c}</figcaption> })}
        </figure>
    }
}
