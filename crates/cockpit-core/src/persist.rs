//! Saving and loading the [`Workspace`] as text (JSON) – without browser and without UI.
//!
//! The backend stores the text in SQLite; this only describes what it looks like and how
//! it is read back safely:
//!
//! * The text carries a **version number** ([`VERSION`]). Whoever changes the shape of the data
//!   incompatibly raises it – old states are then discarded.
//! * **New fields** in the types are uncritical: old data without the field gets
//!   the default value (`#[serde(default)]`), unknown fields are ignored. New
//!   fields therefore get this attribute, as long as their type implements `Default`.
//! * **Unreadable data** never leads to a crash: [`restore`] falls back to the sample data.
//!
//! Keys and tokens are not part of the workspace and never end up here.

use serde::{Deserialize, Serialize};

use crate::{Workspace, demo};

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
    /// Server mode only: number of the state (counts with every applied action).
    #[serde(skip_serializing_if = "Option::is_none")]
    revision: Option<u64>,
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

/// Turns the workspace into text.
pub fn save(workspace: &Workspace) -> String {
    to_text(workspace, None)
}

/// Like [`save`], with the server's revision number. The same shape, just one more field;
/// [`load`] still reads the text.
pub fn save_with_revision(workspace: &Workspace, revision: u64) -> String {
    to_text(workspace, Some(revision))
}

fn to_text(workspace: &Workspace, revision: Option<u64>) -> String {
    serde_json::to_string(&Out {
        version: VERSION,
        workspace,
        revision,
    })
    // The workspace contains only texts, numbers and enumerations – this does not fail.
    .unwrap_or_default()
}

/// Reads stored text. Checks the version first, then the content.
pub fn load(text: &str) -> Result<Workspace, LoadError> {
    load_with_revision(text).map(|(workspace, _)| workspace)
}

/// Like [`load`], plus the revision number (0 if the text contains none).
pub fn load_with_revision(text: &str) -> Result<(Workspace, u64), LoadError> {
    let header: Header = serde_json::from_str(text).map_err(|_| LoadError::Unreadable)?;
    if header.version != VERSION {
        return Err(LoadError::Version(header.version));
    }
    serde_json::from_str::<In>(text)
        .map(|saved| (saved.workspace, saved.revision))
        .map_err(|_| LoadError::Unreadable)
}

/// The workspace for the start: the stored state, otherwise the sample data.
/// The second value says whether the stored state was used.
pub fn restore(saved: Option<&str>) -> (Workspace, bool) {
    match saved.map(load) {
        Some(Ok(workspace)) => (workspace, true),
        _ => (demo::seed(), false),
    }
}

#[cfg(test)]
mod tests;
