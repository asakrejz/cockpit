//! Standard-UI-Bausteine: `Button`, `Link` und `Image`.
//!
//! Das Aussehen wird über Props gesteuert (Varianten, Größen, Zustände).
//! Die Props sind bewusst typisiert (Enums statt Strings), damit der
//! Compiler Tippfehler wie `variant="primray"` abfängt.
//! Styles liegen in `style.css` und nutzen CSS-Variablen mit Fallback-Werten.

mod button;
mod image;
mod link;

pub use button::{Button, ButtonVariant};
pub use image::{Image, ImageFit, ImageRadius};
pub use link::{Link, LinkUnderline, LinkVariant};

/// Gemeinsame Größen für Komponenten.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Size {
    Sm,
    #[default]
    Md,
    Lg,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::Sm, Size::Md, Size::Lg];

    /// Name, wie er auch in der CSS-Klasse vorkommt.
    pub fn name(self) -> &'static str {
        match self {
            Size::Sm => "sm",
            Size::Md => "md",
            Size::Lg => "lg",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_names_are_unique() {
        let names: Vec<_> = Size::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(names, ["sm", "md", "lg"]);
    }

    #[test]
    fn defaults() {
        assert_eq!(Size::default(), Size::Md);
        assert_eq!(ButtonVariant::default(), ButtonVariant::Primary);
    }
}
