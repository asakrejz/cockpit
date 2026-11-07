use feature_temperature::TemperatureConverter;
use leptos::prelude::*;

use super::Story;

pub const STORY: Story = Story {
    id: "temperature",
    title: "Temperatur",
    crate_name: "feature-temperature",
    description: "Zwei Eingabefelder, die sich gegenseitig aktualisieren. Die Umrechnung ist reine, getestete Rust-Logik.",
    render,
};

fn render() -> AnyView {
    view! {
        <TemperatureConverter />
    }
    .into_any()
}
