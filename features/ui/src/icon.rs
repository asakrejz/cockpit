use leptos::prelude::*;

/// Ein Strich-Icon (24×24-Raster, `currentColor`) aus einem SVG-Pfad.
///
/// Fertige Pfade stehen in [`icons`].
#[component]
pub fn Icon(
    #[prop(into)] path: String,
    #[prop(default = 18)] size: u32,
    #[prop(default = 2.0)] stroke: f64,
) -> impl IntoView {
    view! {
        <svg
            width=size
            height=size
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width=stroke
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <path d=path />
        </svg>
    }
}

/// SVG-Pfade für `<Icon path=… />`.
pub mod icons {
    pub const PLUS: &str = "M12 5v14M5 12h14";
    pub const MINUS: &str = "M5 12h14";
    pub const CLOSE: &str = "M6 6l12 12M18 6L6 18";
    pub const CHECK: &str = "M5 12l5 5L20 7";
    pub const ARROW_RIGHT: &str = "M5 12h14M13 6l6 6-6 6";
    pub const CHEVRON_LEFT: &str = "M15 6l-6 6 6 6";
    pub const CHEVRON_RIGHT: &str = "M9 6l6 6-6 6";
    /// Dicker Chevron für Listenzeilen (mit `stroke=3.0` verwenden).
    pub const CHEVRON_ROW: &str = "M9 5l7 7-7 7";
    pub const SEARCH: &str = "M18 11a7 7 0 1 1-14 0 7 7 0 0 1 14 0zM20 20l-3.5-3.5";
    pub const SHARE: &str = "M12 3v12M7 8l5-5 5 5M5 14v5a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-5";
    pub const IMAGE: &str = "M3 7a3 3 0 0 1 3-3h12a3 3 0 0 1 3 3v10a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3zM10.8 10a1.8 1.8 0 1 1-3.6 0 1.8 1.8 0 0 1 3.6 0zM21 16l-5-5-9 9";
    pub const SUN: &str = "M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0zM12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4";
    pub const DOT: &str = "M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0z";
    pub const MOON: &str = "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z";
    pub const AIRPLANE: &str = "M2 16l20-6-20-6 4 6-4 6zM6 10h8";
    pub const WIFI: &str =
        "M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01M2 9a14 14 0 0 1 20 0";
    pub const BLUETOOTH: &str = "M7 7l10 10-5 5V2l5 5L7 17";
    pub const BELL: &str = "M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10 21h4";
    pub const HOME: &str = "M3 11l9-8 9 8v9a1 1 0 0 1-1 1h-5v-6h-6v6H4a1 1 0 0 1-1-1z";
    pub const LIBRARY: &str = "M4 4h4v16H4zM10 4h4v16h-4zM16 5l4 1-3 14-4-1z";
    pub const SETTINGS: &str = "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-2.9-1.2l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.7 1.7 0 0 0 3 14H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.2-2.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 10 3V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 2.9 1.2l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1A1.7 1.7 0 0 0 21 10h0a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z";
    pub const TASKS: &str = "M9 12l2 2 4-4M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0z";
    pub const USERS: &str = "M16 20v-1a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v1M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM22 20v-1a4 4 0 0 0-3-3.9M16 3.1a4 4 0 0 1 0 7.8";
    pub const CHART: &str = "M5 20V11M12 20V4M19 20v-6";
    pub const GIT_PR: &str = "M6 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 9v12M18 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 15V9a3 3 0 0 0-3-3h-3";
    pub const GIT_BRANCH: &str = "M6 3v12M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9a9 9 0 0 1-9 9";
    /// Zwei Funken – das Zeichen für den Agenten.
    pub const SPARKLES: &str = "M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9zM19 16l.8 2.2L22 19l-2.2.8L19 22l-.8-2.2L16 19l2.2-.8z";
    pub const SPARKLE: &str = "M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z";
    pub const SLIDERS: &str = "M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M16 4v4M10 10v4M18 16v4";
    pub const COMMENT: &str = "M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z";
    pub const EYE: &str =
        "M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z";
    pub const ALERT: &str = "M12 8v5M12 16.5h.01M10.3 3.9L2.5 17.5a2 2 0 0 0 1.7 3h15.6a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z";
    pub const ASSIGN: &str = "M16 20v-1a4 4 0 0 0-4-4H7a4 4 0 0 0-4 4v1M9.5 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM19 8v6M22 11h-6";
    /// Vier Kacheln – Marken-Platzhalter für die Navbar.
    pub const BRAND: &str = "M3 5.5A2.5 2.5 0 0 1 5.5 3h3A2.5 2.5 0 0 1 11 5.5v3A2.5 2.5 0 0 1 8.5 11h-3A2.5 2.5 0 0 1 3 8.5zM21 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0zM11 17a4 4 0 1 1-8 0 4 4 0 0 1 8 0zM13 15.5a2.5 2.5 0 0 1 2.5-2.5h3a2.5 2.5 0 0 1 2.5 2.5v3a2.5 2.5 0 0 1-2.5 2.5h-3a2.5 2.5 0 0 1-2.5-2.5z";
}
