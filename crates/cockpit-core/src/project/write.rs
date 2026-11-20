//! What changes at a GitHub project: the shapes the actions carry.

use serde::{Deserialize, Serialize};

/// A new value for a project field.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum FieldInput {
    /// A choice of a single-select field (the option ID).
    Select(String),
    /// An iteration (the iteration ID).
    Iteration(String),
    Text(String),
    /// A number as typed (`3`, `2.5`).
    Number(String),
    /// A day, `2026-11-20`.
    Date(String),
}

impl FieldInput {
    /// Is the value well-formed for its kind? Text may not be empty (that clears the field), a
    /// number must parse, a date must be `YYYY-MM-DD`.
    pub fn is_valid(&self) -> bool {
        match self {
            FieldInput::Select(id) | FieldInput::Iteration(id) => !id.trim().is_empty(),
            FieldInput::Text(t) => !t.trim().is_empty(),
            FieldInput::Number(n) => n.trim().parse::<f64>().is_ok_and(f64::is_finite),
            FieldInput::Date(d) => {
                let d = d.trim();
                let b = d.as_bytes();
                b.len() == 10
                    && b[4] == b'-'
                    && b[7] == b'-'
                    && b.iter()
                        .enumerate()
                        .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
                    && (1..=12).contains(&d[5..7].parse::<u32>().unwrap_or(0))
                    && (1..=31).contains(&d[8..10].parse::<u32>().unwrap_or(0))
            }
        }
    }
}

/// What GitHub answered when an issue was created or a draft was converted: the backend fills it
/// in, a value sent by the browser is dropped.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CreatedIssue {
    /// `owner/name`
    pub repo: String,
    pub number: u32,
    /// GraphQL node ID of the issue (`I_…`).
    pub content_id: String,
    /// GraphQL node ID of the item in the project (`PVTI_…`).
    pub item_id: String,
    #[serde(default)]
    pub url: String,
}

/// The internal ID of the ticket for an issue: `ghi-acme-app-12`.
///
/// ```
/// use cockpit_core::project::issue_id;
/// assert_eq!(issue_id("acme/app", 12), "ghi-acme-app-12");
/// ```
pub fn issue_id(repo: &str, number: u32) -> String {
    id_from(&["ghi", repo, &number.to_string()])
}

/// The internal ID of the ticket for a draft issue: `ghd-pvti-abc`.
pub fn draft_id(item_id: &str) -> String {
    id_from(&["ghd", item_id])
}

/// Lowercase letters and digits, everything else becomes `-` (once, none at the ends).
fn id_from(parts: &[&str]) -> String {
    let raw = parts.join("-").to_lowercase();
    let mut out = String::new();
    for c in raw.chars() {
        let c = if c.is_alphanumeric() { c } else { '-' };
        if c != '-' || !out.ends_with('-') {
            out.push(c);
        }
    }
    out.trim_matches('-').to_string()
}
