//! UI building blocks following the design "Baukasten" (iOS 26 / Liquid Glass).
//!
//! The crate is a thin Leptos layer over the CSS classes from
//! `components.css`. The API mirrors the SwiftUI modifiers of the design:
//!
//! | SwiftUI                    | Rust                                   | CSS                              |
//! |----------------------------|----------------------------------------|----------------------------------|
//! | `.buttonStyle(.glass)`     | `variant=ButtonStyle::Glass`           | `data-style="glass"`             |
//! | `.controlSize(.large)`     | `size=ControlSize::Large`              | `data-control-size="large"`      |
//! | `role: .destructive`       | `destructive=true`                     | `data-role="destructive"`        |
//!
//! The values (colors, spacing, radii …) come exclusively from the
//! design tokens (`crates/ui/tokens.css` in the workspace). Including it:
//!
//! ```html
//! <link data-trunk rel="css" href="crates/ui/tokens.css" />
//! <link data-trunk rel="css" href="crates/ui/components.css" />
//! <link data-trunk rel="css" href="crates/ui/extras.css" />
//! ```
//!
//! All components must be inside [`Root`] (sets theme, tint, contrast).

mod button;
mod card;
mod code;
mod controls;
mod field;
mod footer;
mod icon;
mod layout;
mod list;
mod nav;
mod pager;
mod progress;
mod root;
mod sheet;
mod shell;
mod stack;
mod state;
mod status;
mod text;
mod thread;

pub use button::*;
pub use card::*;
pub use code::*;
pub use controls::*;
pub use field::*;
pub use footer::*;
pub use icon::*;
pub use layout::*;
pub use list::*;
pub use nav::*;
pub use pager::*;
pub use progress::*;
pub use root::*;
pub use sheet::*;
pub use shell::*;
pub use stack::*;
pub use state::*;
pub use status::*;
pub use text::*;
pub use thread::*;

/// Unique IDs to connect `<label for=…>` with its field.
pub(crate) fn next_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    format!("{prefix}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests;
