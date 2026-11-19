//! Rich text of Jira (Atlassian Document Format, ADF) as a data model.
//!
//! Jira delivers descriptions and comments as an ADF JSON tree. Instead of flattening it to
//! text right away, [`parse`] turns it into the typed tree [`Doc`], which the UI renders
//! with real elements (no HTML from outside ever). Unknown nodes are never an error: they
//! show up as the text of their content.
//!
//! Links are only kept for `http` and `https` ([`safe_url`]); everything else (such as
//! `javascript:`) is shown as plain text.

use serde::{Deserialize, Serialize};

mod parse;

pub use parse::{parse, safe_url};

/// A parsed ADF document: a list of blocks.
pub type Doc = Vec<Block>;

/// Kind of a `panel` (the colored info box of Jira).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PanelKind {
    #[default]
    Info,
    Note,
    Success,
    Warning,
    Error,
}

/// Block element.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Block {
    Paragraph {
        content: Vec<Inline>,
    },
    /// `level` is 1–6.
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    BulletList {
        items: Vec<Vec<Block>>,
    },
    OrderedList {
        #[serde(default = "one")]
        start: u32,
        items: Vec<Vec<Block>>,
    },
    CodeBlock {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        text: String,
    },
    Quote {
        content: Vec<Block>,
    },
    Rule,
    Table {
        rows: Vec<Vec<Cell>>,
    },
    Panel {
        kind: PanelKind,
        content: Vec<Block>,
    },
    /// Attachment or image. `attachment` is the Jira attachment ID when it is an image the app
    /// can show ([`crate::details::link_media`]); otherwise only a placeholder with the file name.
    Media {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachment: Option<String>,
    },
}

fn one() -> u32 {
    1
}

/// A table cell.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// `tableHeader` instead of `tableCell`.
    #[serde(default)]
    pub header: bool,
    pub content: Vec<Block>,
}

/// Formatting of a piece of text.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    #[serde(default, skip_serializing_if = "is_false")]
    pub strong: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub em: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    /// Only `http`/`https` addresses; anything else is dropped when parsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Marks {
    pub fn is_plain(&self) -> bool {
        *self == Marks::default()
    }
}

/// Inline element inside a paragraph or heading.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Inline {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Marks::is_plain")]
        marks: Marks,
    },
    HardBreak,
    /// Visible text with the `@`, e.g. `@Mira Keller` (`@Du` for oneself).
    Mention {
        text: String,
    },
    Emoji {
        text: String,
    },
    /// A smart link; `url` is `http`/`https`, otherwise the card became plain text.
    InlineCard {
        url: String,
    },
    Status {
        text: String,
        #[serde(default)]
        color: String,
    },
    /// Day as `2026-11-09`.
    Date {
        text: String,
    },
}

/// The text of the content of blocks, one line per block. For searching and as a fallback.
pub fn plain_text(doc: &[Block]) -> String {
    let mut out = String::new();
    blocks_text(doc, &mut out);
    out.trim().to_string()
}

fn line_end(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn blocks_text(blocks: &[Block], out: &mut String) {
    for b in blocks {
        match b {
            Block::Paragraph { content } | Block::Heading { content, .. } => {
                line_end(out);
                inlines_text(content, out);
                line_end(out);
            }
            Block::BulletList { items } | Block::OrderedList { items, .. } => {
                for item in items {
                    blocks_text(item, out);
                }
            }
            Block::CodeBlock { text, .. } => {
                line_end(out);
                out.push_str(text);
                line_end(out);
            }
            Block::Quote { content } | Block::Panel { content, .. } => blocks_text(content, out),
            Block::Rule => {}
            Block::Table { rows } => {
                for cell in rows.iter().flatten() {
                    blocks_text(&cell.content, out);
                }
            }
            Block::Media { name, .. } => {
                line_end(out);
                out.push_str(name);
                line_end(out);
            }
        }
    }
}

fn inlines_text(inlines: &[Inline], out: &mut String) {
    for i in inlines {
        match i {
            Inline::Text { text, .. }
            | Inline::Mention { text }
            | Inline::Emoji { text }
            | Inline::Status { text, .. }
            | Inline::Date { text } => out.push_str(text),
            Inline::InlineCard { url } => out.push_str(url),
            Inline::HardBreak => out.push('\n'),
        }
    }
}

#[cfg(test)]
mod tests;
