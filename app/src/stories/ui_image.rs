use feature_ui::{Image, ImageFit, ImageRadius};
use leptos::prelude::*;

use super::Story;

pub const STORY: Story = Story {
    id: "image",
    title: "Image",
    crate_name: "feature-ui",
    description: "Zuschnitt (fit), Rundung, Seitenverhältnis, Bildunterschrift und Fallback bei Ladefehlern.",
    render,
};

/// Erzeugt ein Beispielbild als SVG-Data-URI (kein Netzwerk nötig).
/// `#` muss als `%23` kodiert werden.
fn demo_svg(w: u32, h: u32) -> String {
    format!(
        "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}' viewBox='0 0 {w} {h}' preserveAspectRatio='none'>\
<defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'>\
<stop offset='0' stop-color='%23fb923c'/><stop offset='1' stop-color='%23be185d'/></linearGradient></defs>\
<rect width='100%25' height='100%25' fill='url(%23g)'/>\
<circle cx='{cx}' cy='{cy}' r='{r}' fill='white' fill-opacity='0.85'/>\
<text x='50%25' y='50%25' font-size='{fs}' text-anchor='middle' dominant-baseline='central' fill='%231c1917'>{w}x{h}</text>\
</svg>",
        cx = w / 2,
        cy = h / 2,
        r = w.min(h) / 3,
        fs = w.min(h) / 8,
    )
}

fn render() -> AnyView {
    let wide = demo_svg(400, 200);

    view! {
        <h3>"fit (Rahmen 160×160, Bild 400×200)"</h3>
        <div class="row">
            {ImageFit::ALL
                .map(|fit| {
                    view! {
                        <Image
                            src=wide.clone()
                            alt=format!("Beispielbild mit fit={}", fit.name())
                            width=160
                            ratio="1"
                            fit=fit
                            caption=fit.name()
                        />
                    }
                })
                .into_iter()
                .collect_view()}
        </div>

        <h3>"radius"</h3>
        <div class="row">
            {ImageRadius::ALL
                .map(|radius| {
                    view! {
                        <Image
                            src=demo_svg(200, 200)
                            alt=format!("Beispielbild mit radius={}", radius.name())
                            width=120
                            ratio="1"
                            radius=radius
                            caption=radius.name()
                        />
                    }
                })
                .into_iter()
                .collect_view()}
        </div>

        <h3>"Seitenverhältnis 16 / 9"</h3>
        <Image
            src=demo_svg(640, 360)
            alt="Beispielbild im Format 16 zu 9"
            width=320
            ratio="16 / 9"
            radius=ImageRadius::Md
            caption="ratio=\"16 / 9\""
        />

        <h3>"Fallback bei Ladefehler"</h3>
        <Image
            src="/gibt-es-nicht.png"
            alt="Dieses Bild existiert nicht"
            width=240
            ratio="16 / 9"
            radius=ImageRadius::Md
        />
    }
    .into_any()
}
