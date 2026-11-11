use std::fmt;

/// Ein Zugangsschlüssel. `Debug` zeigt ihn nie, es gibt kein `Display` und kein `Serialize`.
/// Lesen geht nur über [`Secret::expose`] – dort, wo er in einen Header geschrieben wird.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(wert: impl Into<String>) -> Self {
        Self(wert.into())
    }

    /// Der Klartext. Nur für den Aufruf beim Dienst verwenden, nie loggen.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_zeigt_den_schluessel_nie() {
        let s = Secret::new("ghp_geheim");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(format!("{:#?}", Some(&s)), "Some(\n    Secret(***),\n)");
        assert_eq!(s.expose(), "ghp_geheim");
    }
}
