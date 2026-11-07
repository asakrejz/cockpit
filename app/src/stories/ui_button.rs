use feature_ui::{Button, ButtonVariant, Size};
use leptos::prelude::*;

use super::Story;

pub const STORY: Story = Story {
    id: "button",
    title: "Button",
    crate_name: "feature-ui",
    description: "Varianten, Größen und Zustände – alles über Props gesteuert. Unten kannst du die Props live ändern.",
    render,
};

fn render() -> AnyView {
    // Zustand des Bedienfelds ("Controls" wie in Storybook).
    let variant = RwSignal::new(ButtonVariant::Primary);
    let size = RwSignal::new(Size::Md);
    let disabled = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let full_width = RwSignal::new(false);
    let label = RwSignal::new("Klick mich".to_string());
    let clicks = RwSignal::new(0);

    view! {
        <h3>"Varianten"</h3>
        <div class="row">
            {ButtonVariant::ALL
                .map(|v| view! { <Button variant=v>{v.name()}</Button> })
                .into_iter()
                .collect_view()}
        </div>

        <h3>"Größen"</h3>
        <div class="row">
            {Size::ALL.map(|s| view! { <Button size=s>{s.name()}</Button> }).into_iter().collect_view()}
        </div>

        <h3>"Zustände"</h3>
        <div class="row">
            <Button disabled=true>"disabled"</Button>
            <Button loading=true>"loading"</Button>
            <Button variant=ButtonVariant::Outline disabled=true>"outline disabled"</Button>
        </div>

        <h3>"Volle Breite"</h3>
        <Button full_width=true variant=ButtonVariant::Secondary>"full_width"</Button>

        <h3>"Live-Controls"</h3>
        <div class="controls">
            <label>
                "variant "
                <select on:change=move |ev| {
                    if let Some(v) = ButtonVariant::from_name(&event_target_value(&ev)) {
                        variant.set(v);
                    }
                }>
                    {ButtonVariant::ALL
                        .map(|v| view! { <option value=v.name()>{v.name()}</option> })
                        .into_iter()
                        .collect_view()}
                </select>
            </label>
            <label>
                "size "
                <select
                    prop:value="md"
                    on:change=move |ev| {
                        let name = event_target_value(&ev);
                        if let Some(s) = Size::ALL.into_iter().find(|s| s.name() == name) {
                            size.set(s);
                        }
                    }
                >
                    {Size::ALL
                        .map(|s| view! { <option value=s.name()>{s.name()}</option> })
                        .into_iter()
                        .collect_view()}
                </select>
            </label>
            <label>
                <input type="checkbox" on:change=move |ev| disabled.set(event_target_checked(&ev)) />
                " disabled"
            </label>
            <label>
                <input type="checkbox" on:change=move |ev| loading.set(event_target_checked(&ev)) />
                " loading"
            </label>
            <label>
                <input type="checkbox" on:change=move |ev| full_width.set(event_target_checked(&ev)) />
                " full_width"
            </label>
            <label>
                "Text "
                <input
                    type="text"
                    prop:value=label
                    on:input=move |ev| label.set(event_target_value(&ev))
                />
            </label>
        </div>
        // `full_width` ist ein normales (nicht-reaktives) Prop – daher die Komponente
        // bei Änderung neu aufbauen.
        <div class="preview">
            {move || {
                view! {
                    <Button
                        variant=variant
                        size=size
                        disabled=disabled
                        loading=loading
                        full_width=full_width.get()
                        on_click=move |_| clicks.update(|c| *c += 1)
                    >
                        {label.get()}
                    </Button>
                }
            }}
            <p class="hint">"Klicks: " {clicks}</p>
        </div>
    }
    .into_any()
}
