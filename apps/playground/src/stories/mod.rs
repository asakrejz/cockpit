//! Registry aller Stories.
//!
//! Eine *Story* ist die Präsentation eines Bausteins im Playground.
//! Der Baustein selbst lebt im Crate unter `crates/` und weiß nichts
//! vom Playground – so bleibt er wiederverwendbar.

mod app_blocks;
mod button;
mod common;
mod controls;
mod fields;
mod foundations;
mod layout;
mod link_badge;
mod list;
mod media_card;
mod navigation;
mod pager;
mod phone;

use leptos::prelude::AnyView;

pub struct Story {
    /// Eindeutige ID, erscheint in der URL (`#id`).
    pub id: &'static str,
    /// Name des Reiters.
    pub title: &'static str,
    /// Überschrift in der Navigation.
    pub group: &'static str,
    pub description: &'static str,
    /// Klassen und Attribute, um die es geht (Kurzreferenz).
    pub api: &'static str,
    /// Rendert die Story.
    pub render: fn() -> AnyView,
}

/// 👉 Neue Stories hier eintragen – die Reihenfolge bestimmt die Navigation.
/// Stories derselben Gruppe stehen direkt hintereinander.
pub static STORIES: &[Story] = &[
    foundations::STORY,
    button::STORY,
    link_badge::STORY,
    controls::STORY,
    fields::STORY,
    list::STORY,
    media_card::STORY,
    navigation::STORY,
    pager::STORY,
    layout::STORY,
    phone::STORY,
    app_blocks::STORY,
];

pub fn find(id: &str) -> Option<&'static Story> {
    STORIES.iter().find(|s| s.id == id)
}

/// Stories nach Gruppen für die Navigation.
pub fn groups() -> Vec<(&'static str, Vec<&'static Story>)> {
    let mut groups: Vec<(&'static str, Vec<&'static Story>)> = Vec::new();
    for story in STORIES {
        match groups.iter_mut().find(|(name, _)| *name == story.group) {
            Some((_, items)) => items.push(story),
            None => groups.push((story.group, vec![story])),
        }
    }
    groups
}
