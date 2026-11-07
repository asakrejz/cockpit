use feature_ui::{Link, LinkUnderline, LinkVariant};
use leptos::prelude::*;

use super::Story;

pub const STORY: Story = Story {
    id: "link",
    title: "Link",
    crate_name: "feature-ui",
    description: "Interne und externe Links, Varianten, Unterstreichung und deaktivierter Zustand.",
    render,
};

fn render() -> AnyView {
    view! {
        <h3>"Varianten"</h3>
        <div class="row">
            {LinkVariant::ALL
                .map(|v| view! { <Link href="#link" variant=v>{v.name()}</Link> })
                .into_iter()
                .collect_view()}
        </div>

        <h3>"Unterstreichung"</h3>
        <div class="row">
            {LinkUnderline::ALL
                .map(|u| view! { <Link href="#link" underline=u>{u.name()}</Link> })
                .into_iter()
                .collect_view()}
        </div>

        <h3>"Extern (neuer Tab, rel=noopener)"</h3>
        <div class="row">
            <Link href="https://www.rust-lang.org" external=true>"rust-lang.org"</Link>
            <Link href="https://leptos.dev" external=true variant=LinkVariant::Button>
                "Leptos Docs"
            </Link>
        </div>

        <h3>"Deaktiviert"</h3>
        <Link href="#link" disabled=true>"nicht anklickbar"</Link>

        <h3>"Im Fließtext"</h3>
        <p>
            "Rust kompiliert zu "
            <Link href="https://webassembly.org" external=true underline=LinkUnderline::Always>
                "WebAssembly"
            </Link>
            ", das im Browser läuft."
        </p>
    }
    .into_any()
}
