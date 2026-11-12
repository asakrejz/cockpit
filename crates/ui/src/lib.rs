//! UI-Bausteine nach dem Design "Baukasten" (iOS 26 / Liquid Glass).
//!
//! Das Crate ist eine dünne Leptos-Schicht über den CSS-Klassen aus
//! `components.css`. Die API spiegelt die SwiftUI-Modifier des Designs:
//!
//! | SwiftUI                    | Rust                                   | CSS                              |
//! |----------------------------|----------------------------------------|----------------------------------|
//! | `.buttonStyle(.glass)`     | `variant=ButtonStyle::Glass`           | `data-style="glass"`             |
//! | `.controlSize(.large)`     | `size=ControlSize::Large`              | `data-control-size="large"`      |
//! | `role: .destructive`       | `destructive=true`                     | `data-role="destructive"`        |
//!
//! Die Werte (Farben, Abstände, Radien …) kommen ausschließlich aus den
//! Design-Tokens (`crates/ui/tokens.css` im Workspace). Einbinden:
//!
//! ```html
//! <link data-trunk rel="css" href="crates/ui/tokens.css" />
//! <link data-trunk rel="css" href="crates/ui/components.css" />
//! <link data-trunk rel="css" href="crates/ui/extras.css" />
//! ```
//!
//! Alle Komponenten müssen innerhalb von [`Root`] stehen (setzt Theme, Tint, Kontrast).

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
pub use status::*;
pub use text::*;
pub use thread::*;

/// Eindeutige IDs, um `<label for=…>` mit seinem Feld zu verbinden.
pub(crate) fn next_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    format!("{prefix}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn all_unique(ids: impl IntoIterator<Item = &'static str>) -> bool {
        let ids: Vec<_> = ids.into_iter().collect();
        ids.iter().collect::<HashSet<_>>().len() == ids.len()
    }

    #[test]
    fn enum_ids_are_unique() {
        assert!(all_unique(ButtonStyle::ALL.map(ButtonStyle::id)));
        assert!(all_unique(ControlSize::ALL.map(ControlSize::id)));
        assert!(all_unique(ButtonShape::ALL.map(ButtonShape::id)));
        assert!(all_unique(Tint::ALL.map(Tint::id)));
        assert!(all_unique(TextStyle::ALL.map(TextStyle::id)));
        assert!(all_unique(MediaRatio::ALL.map(MediaRatio::id)));
    }

    /// Standardwerte lassen ihr Attribut weg – das CSS definiert dafür keine Regel.
    #[test]
    fn defaults_omit_their_attribute() {
        assert_eq!(ControlSize::default().attr(), None);
        assert_eq!(ButtonShape::default().attr(), None);
        assert_eq!(BadgeVariant::default().attr(), None);
        assert_eq!(Contrast::default().attr(), None);
        assert_eq!(ControlSize::Large.attr(), Some("large"));
        assert_eq!(Contrast::High.attr(), Some("high"));
    }

    /// Die IDs müssen zu den Selektoren in `components.css` passen.
    #[test]
    fn ids_match_css_selectors() {
        let css = include_str!("../components.css");
        for style in ButtonStyle::ALL {
            // `bordered` ist der Standard und hat keine eigene Regel.
            if style != ButtonStyle::Bordered {
                assert!(
                    css.contains(&format!("data-style=\"{}\"", style.id())),
                    "kein CSS für data-style={}",
                    style.id()
                );
            }
        }
        for size in ControlSize::ALL {
            if let Some(id) = size.attr() {
                assert!(
                    css.contains(&format!("data-control-size=\"{id}\"")),
                    "kein CSS für {id}"
                );
            }
        }
        for ratio in MediaRatio::ALL {
            if ratio != MediaRatio::R16x9 {
                assert!(css.contains(&format!("data-ratio=\"{}\"", ratio.id())));
            }
        }
        for tone in StatusTone::ALL {
            if let Some(id) = tone.attr() {
                assert!(
                    css.contains(&format!("data-tone=\"{id}\"")),
                    "kein CSS für Ton {id}"
                );
            }
        }
        for style in TextStyle::ALL {
            assert!(
                css.contains(&format!(".t-{} ", style.id())),
                "kein CSS für .t-{}",
                style.id()
            );
        }
    }
}
