//! Saving and loading the [`Workspace`] as text (JSON).
//!
//! * The text carries a **version number** ([`VERSION`]). Whoever changes the shape of the data
//!   incompatibly raises it; text with another version is rejected as [`LoadError::Version`].
//! * **New fields** in the types are uncritical: old data without the field gets
//!   the default value (`#[serde(default)]`), unknown fields are ignored. New
//!   fields therefore get this attribute, as long as their type implements `Default`.
//! * **Unreadable data** never leads to a crash: [`load_with_revision`] reports why it cannot be used.
//!
//! Keys and tokens are not part of the workspace and never end up here.

use serde::{Deserialize, Serialize};

use crate::Workspace;

/// Version of the storage shape. Raise on incompatible changes to the data model.
pub const VERSION: u32 = 1;

/// Only the version number – so that it can be read even when the rest does not fit.
#[derive(Deserialize)]
struct Header {
    version: u32,
}

#[derive(Serialize)]
struct Out<'a> {
    version: u32,
    workspace: &'a Workspace,
    /// Number of the state (counts with every applied action).
    revision: u64,
}

#[derive(Deserialize)]
struct In {
    workspace: Workspace,
    #[serde(default)]
    revision: u64,
}

/// Why stored data is not used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// Not valid JSON or not in the expected shape.
    Unreadable,
    /// Comes from a different version of the app.
    Version(u32),
}

/// Turns the workspace into text, with the number of the state.
pub fn save_with_revision(workspace: &Workspace, revision: u64) -> String {
    serde_json::to_string(&Out {
        version: VERSION,
        workspace,
        revision,
    })
    // The workspace contains only texts, numbers and enumerations – this does not fail.
    .unwrap_or_default()
}

/// Reads stored text: the workspace and its revision number (0 if the text contains none).
/// Checks the version first, then the content.
pub fn load_with_revision(text: &str) -> Result<(Workspace, u64), LoadError> {
    let header: Header = serde_json::from_str(text).map_err(|_| LoadError::Unreadable)?;
    if header.version != VERSION {
        return Err(LoadError::Version(header.version));
    }
    serde_json::from_str::<In>(text)
        .map(|saved| (saved.workspace, saved.revision))
        .map_err(|_| LoadError::Unreadable)
}

#[cfg(test)]
mod tests;
