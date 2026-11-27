//! More of a Jira ticket than its description: the fields (standard and custom) and the
//! attachments, as the sync reads them.
//!
//! The fields arrive ready to show: empty ones are already gone, the order is settled and
//! rich text (ADF fields such as acceptance criteria) is parsed. Standard fields are identified
//! by [`DetailField::id`]; [`DetailField::name`] is the name as Jira gives it.

use serde::{Deserialize, Serialize};

use crate::adf::Doc;

/// What the details of a ticket hold.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    #[serde(default)]
    pub fields: Vec<DetailField>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

impl Details {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.attachments.is_empty()
    }
}

/// One non-empty field.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DetailField {
    /// Jira's ID: `status`, `labels`, `customfield_10042` …
    pub id: String,
    /// The name Jira gives the field (`expand=names`), in the language of the site.
    #[serde(default)]
    pub name: String,
    pub value: DetailValue,
}

/// The value of a field.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum DetailValue {
    Text(String),
    List(Vec<String>),
    /// A day or point in time as `2026-11-09`.
    Date(String),
    /// A field of formatted text (ADF).
    Rich(Doc),
}

/// An attachment of a ticket.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    /// Jira's attachment ID (digits).
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Size in bytes.
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mime: String,
    /// Jira made a thumbnail.
    #[serde(default)]
    pub thumbnail: bool,
}

/// Image types the app shows inline. SVG is left out on purpose: it can carry scripts.
pub const INLINE_IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

impl Attachment {
    /// May the browser show it inline?
    pub fn is_image(&self) -> bool {
        is_inline_image(&self.mime)
    }
}

/// Is this media type one the app shows inline?
pub fn is_inline_image(mime: &str) -> bool {
    let base = mime.split(';').next().unwrap_or("").trim().to_lowercase();
    INLINE_IMAGE_TYPES.contains(&base.as_str())
}

/// Hooks media nodes of a description or comment up with the attachments: an image that an
/// attachment of the same file name belongs to gets that attachment's ID.
pub fn link_media(doc: &mut Doc, attachments: &[Attachment]) {
    use crate::adf::Block;
    fn go(blocks: &mut [Block], attachments: &[Attachment]) {
        for b in blocks {
            match b {
                Block::Media { name, attachment } => {
                    *attachment = attachments
                        .iter()
                        .find(|a| a.is_image() && a.name.eq_ignore_ascii_case(name))
                        .map(|a| a.id.clone());
                }
                Block::BulletList { items } | Block::OrderedList { items, .. } => {
                    for i in items {
                        go(i, attachments);
                    }
                }
                Block::Quote { content } | Block::Panel { content, .. } => go(content, attachments),
                Block::Table { rows } => {
                    for c in rows.iter_mut().flatten() {
                        go(&mut c.content, attachments);
                    }
                }
                _ => {}
            }
        }
    }
    go(doc, attachments);
}

/// The ID of the picture in the sample data.
pub const DEMO_IMAGE_ID: &str = "900001";

/// `1.4 MB`, `812 B`: sizes for people (English units; the same in every language).
pub fn size_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{:.1} MB", b / (KB * KB))
    }
}

/// Where a ticket lives in Jira: `<site>/browse/<KEY>`. Only for `http(s)` sites and keys
/// of the usual shape; `None` otherwise.
pub fn browse_url(site: &str, key: &str) -> Option<String> {
    let site = crate::adf::safe_url(site.trim())?.trim_end_matches('/');
    let valid_key = key.split_once('-').is_some_and(|(p, n)| {
        crate::is_project_key(p) && !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())
    });
    valid_key.then(|| format!("{site}/browse/{key}"))
}

#[cfg(test)]
mod tests;
