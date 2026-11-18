//! The two levels of a list-and-detail page on a narrow screen (like an iOS navigation stack).
//!
//! Which level is shown comes from the address of the page, so the browser's back button and a
//! reload work. This file holds the pure part: the levels and the direction of the slide.

/// The level of a page that is on screen on a narrow device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StackLevel {
    /// The list (or the picker): the first level.
    #[default]
    List,
    /// The detail of one entry: the second level.
    Detail,
}

impl StackLevel {
    /// Value of the `data-level` attribute; the CSS selects on it.
    pub fn attr(self) -> &'static str {
        match self {
            StackLevel::List => "list",
            StackLevel::Detail => "detail",
        }
    }

    /// The slide that leads from `self` to `to`: `forward` pushes the detail in from the right,
    /// `back` brings the list back from the left. `None` when the level does not change.
    pub fn motion_to(self, to: StackLevel) -> Option<&'static str> {
        match (self, to) {
            (StackLevel::List, StackLevel::Detail) => Some("forward"),
            (StackLevel::Detail, StackLevel::List) => Some("back"),
            _ => None,
        }
    }
}

/// How far a touch has to travel from the left edge to count as "back", in pixels.
pub const EDGE_SWIPE_MIN: f64 = 72.0;
/// A touch must start within this many pixels of the left edge.
pub const EDGE_SWIPE_ZONE: f64 = 24.0;

/// True if a touch from `(x0, y0)` to `(x1, y1)` is a swipe from the left edge to the right:
/// it starts in the edge zone, goes far enough and is more horizontal than vertical.
pub fn is_back_swipe(x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    let (dx, dy) = (x1 - x0, (y1 - y0).abs());
    x0 <= EDGE_SWIPE_ZONE && dx >= EDGE_SWIPE_MIN && dx > dy * 2.0
}

#[cfg(test)]
mod tests;
