//! Registry aller Stories.
//!
//! A *story* is the presentation of a building block in the playground.
//! The building block itself lives in the crate under `crates/` and knows nothing
//! about the playground – so it stays reusable.

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
    /// Unique ID, appears in the URL (`#id`).
    pub id: &'static str,
    /// Name of the tab.
    pub title: &'static str,
    /// Heading in the navigation.
    pub group: &'static str,
    pub description: &'static str,
    /// Classes and attributes that it is about (short reference).
    pub api: &'static str,
    /// Renders the story.
    pub render: fn() -> AnyView,
}

/// 👉 Register new stories here – the order determines the navigation.
/// Stories of the same group follow each other directly.
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

/// Stories by groups for the navigation.
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
