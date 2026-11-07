//! Registry aller Stories.
//!
//! Eine *Story* ist die Präsentation eines Features im Playground.
//! Das Feature selbst lebt in seinem eigenen Crate unter `features/`
//! und weiß nichts vom Playground – so bleibt es wiederverwendbar.

mod counter;
mod temperature;
mod todo;
mod ui_button;
mod ui_image;
mod ui_link;

use leptos::prelude::AnyView;

pub struct Story {
    /// Eindeutige ID, erscheint in der URL (`#id`).
    pub id: &'static str,
    /// Name des Reiters.
    pub title: &'static str,
    /// Aus welchem Crate das Feature kommt.
    pub crate_name: &'static str,
    pub description: &'static str,
    /// Rendert die Story.
    pub render: fn() -> AnyView,
}

/// 👉 Neue Stories hier eintragen – die Reihenfolge bestimmt die Navigation.
pub static STORIES: &[Story] = &[
    counter::STORY,
    temperature::STORY,
    todo::STORY,
    ui_button::STORY,
    ui_link::STORY,
    ui_image::STORY,
];

pub fn find(id: &str) -> Option<&'static Story> {
    STORIES.iter().find(|s| s.id == id)
}
