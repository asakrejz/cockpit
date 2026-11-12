use std::fmt;

/// An access key. `Debug` never shows it; there is no `Display` and no `Serialize`.
/// Reading is only possible through [`Secret::expose`] – where it is written into a header.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The plain text. Use only for the call to the service, never log it.
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
    fn debug_never_shows_the_key() {
        let s = Secret::new("ghp_geheim");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(format!("{:#?}", Some(&s)), "Some(\n    Secret(***),\n)");
        assert_eq!(s.expose(), "ghp_geheim");
    }
}
