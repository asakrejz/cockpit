use feature_todo::TodoApp;
use leptos::prelude::*;

use super::Story;

pub const STORY: Story = Story {
    id: "todo",
    title: "Todo-Liste",
    crate_name: "feature-todo",
    description: "Formular, Listen-Rendering mit <For> und ein eigenes Datenmodell mit Unit-Tests.",
    render,
};

fn render() -> AnyView {
    let initial = vec![
        "Rust lernen".to_string(),
        "WebAssembly verstehen".to_string(),
    ];
    view! {
        <TodoApp initial />
    }
    .into_any()
}
