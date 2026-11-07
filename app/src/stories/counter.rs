use feature_counter::Counter;
use leptos::prelude::*;

use super::Story;

pub const STORY: Story = Story {
    id: "counter",
    title: "Counter",
    crate_name: "feature-counter",
    description: "Signals und Props: ein Zähler, einmal mit Standardwerten und einmal konfiguriert.",
    render,
};

fn render() -> AnyView {
    view! {
        <h3>"Standard"</h3>
        <Counter />

        <h3>"initial=100, step=10"</h3>
        <Counter initial=100 step=10 />
    }
    .into_any()
}
