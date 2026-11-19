//! GitHub-flavored Markdown into a small, safe tree.
//!
//! `pulldown-cmark` produces the events (tables, task lists and strikethrough switched on).
//! Raw HTML never reaches the tree as HTML: HTML comments are dropped, any other raw HTML
//! becomes plain text. Links are limited to `http`, `https` and `mailto`; images are
//! reduced to a link, nothing is loaded from outside.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag};

/// A block of the document.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    /// Level 1–6.
    Heading(u8, Vec<Inline>),
    Code {
        language: String,
        text: String,
    },
    Quote(Vec<Block>),
    List {
        /// `Some(n)` for an ordered list starting at `n`.
        start: Option<u64>,
        items: Vec<Item>,
    },
    Rule,
    Table(Table),
}

/// One list item; `checked` is set for task list items (`- [ ]` / `- [x]`).
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

/// Horizontal alignment of a table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub align: Vec<Align>,
    pub head: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
}

/// Text inside a block.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    Strong(Vec<Inline>),
    Em(Vec<Inline>),
    Strike(Vec<Inline>),
    /// `url` is `None` if the address is not allowed: then only the content is shown.
    Link {
        url: Option<String>,
        content: Vec<Inline>,
    },
    /// An image is only ever a link to its address (`url` is `None` if not allowed).
    Image {
        url: Option<String>,
        alt: String,
    },
    /// A line break inside a paragraph (GitHub shows these in PR text).
    Break,
}

/// The address if it may be used as a link target: `http`, `https` or `mailto` only.
pub fn safe_link(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let ok = ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme));
    (ok && !url.chars().any(char::is_control)).then(|| url.to_string())
}

#[derive(Debug)]
enum Kind {
    Doc,
    Quote,
    List(Option<u64>),
    Item,
    Paragraph,
    Heading(u8),
    Strong,
    Em,
    Strike,
    Link(Option<String>),
    Image(Option<String>),
    Code(String),
    Html,
    Table(Vec<Align>),
    Head,
    Row,
    Cell,
}

struct Frame {
    kind: Kind,
    blocks: Vec<Block>,
    inlines: Vec<Inline>,
    items: Vec<Item>,
    checked: Option<bool>,
    text: String,
    rows: Vec<Vec<Vec<Inline>>>,
    cells: Vec<Vec<Inline>>,
}

impl Frame {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            blocks: Vec::new(),
            inlines: Vec::new(),
            items: Vec::new(),
            checked: None,
            text: String::new(),
            rows: Vec::new(),
            cells: Vec::new(),
        }
    }

    /// Loose text in a tight list item or quote becomes a paragraph before the next block.
    fn flush(&mut self) {
        if !self.inlines.is_empty() {
            self.blocks
                .push(Block::Paragraph(std::mem::take(&mut self.inlines)));
        }
    }

    fn push_block(&mut self, block: Block) {
        self.flush();
        self.blocks.push(block);
    }
}

fn align(a: Alignment) -> Align {
    match a {
        Alignment::None => Align::None,
        Alignment::Left => Align::Left,
        Alignment::Center => Align::Center,
        Alignment::Right => Align::Right,
    }
}

fn plain(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for i in inlines {
        match i {
            Inline::Text(t) | Inline::Code(t) => out.push_str(t),
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::Break => out.push(' '),
            Inline::Strong(c)
            | Inline::Em(c)
            | Inline::Strike(c)
            | Inline::Link { content: c, .. } => {
                out.push_str(&plain(c));
            }
        }
    }
    out
}

fn is_comment(html: &str) -> bool {
    html.trim_start().starts_with("<!--")
}

/// Parse Markdown into blocks.
pub fn parse(source: &str) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut stack = vec![Frame::new(Kind::Doc)];
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => {
                let kind = match tag {
                    Tag::Paragraph => Kind::Paragraph,
                    Tag::Heading { level, .. } => Kind::Heading(level as u8),
                    Tag::BlockQuote(_) => Kind::Quote,
                    Tag::CodeBlock(CodeBlockKind::Fenced(lang)) => {
                        Kind::Code(lang.split_whitespace().next().unwrap_or("").to_string())
                    }
                    Tag::CodeBlock(CodeBlockKind::Indented) => Kind::Code(String::new()),
                    Tag::HtmlBlock => Kind::Html,
                    Tag::List(start) => Kind::List(start),
                    Tag::Item => Kind::Item,
                    Tag::Emphasis => Kind::Em,
                    Tag::Strong => Kind::Strong,
                    Tag::Strikethrough => Kind::Strike,
                    Tag::Link { dest_url, .. } => Kind::Link(safe_link(&dest_url)),
                    Tag::Image { dest_url, .. } => Kind::Image(safe_link(&dest_url)),
                    Tag::Table(a) => Kind::Table(a.into_iter().map(align).collect()),
                    Tag::TableHead => Kind::Head,
                    Tag::TableRow => Kind::Row,
                    Tag::TableCell => Kind::Cell,
                    // Everything else (footnotes, metadata, …) only passes its content on.
                    _ => Kind::Quote,
                };
                // A block starting inside a tight item or quote ends the loose text before it.
                if matches!(
                    kind,
                    Kind::Paragraph
                        | Kind::Heading(_)
                        | Kind::Quote
                        | Kind::Code(_)
                        | Kind::Html
                        | Kind::List(_)
                        | Kind::Table(_)
                ) && let Some(top) = stack.last_mut()
                {
                    top.flush();
                }
                stack.push(Frame::new(kind));
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    continue;
                }
                let mut frame = stack.pop().expect("checked above");
                let kind = std::mem::replace(&mut frame.kind, Kind::Doc);
                let parent = stack.last_mut().expect("document frame");
                match kind {
                    Kind::Paragraph => parent.push_block(Block::Paragraph(frame.inlines)),
                    Kind::Heading(level) => {
                        parent.push_block(Block::Heading(level, frame.inlines));
                    }
                    Kind::Quote => {
                        let mut f = frame;
                        f.flush();
                        parent.push_block(Block::Quote(f.blocks));
                    }
                    Kind::Code(language) => {
                        let text = frame.text.strip_suffix('\n').unwrap_or(&frame.text);
                        parent.push_block(Block::Code {
                            language,
                            text: text.to_string(),
                        });
                    }
                    Kind::Html => {
                        if !is_comment(&frame.text) {
                            parent.push_block(Block::Code {
                                language: String::new(),
                                text: frame.text.trim_end().to_string(),
                            });
                        }
                    }
                    Kind::List(start) => parent.push_block(Block::List {
                        start,
                        items: frame.items,
                    }),
                    Kind::Item => {
                        let mut f = frame;
                        f.flush();
                        parent.items.push(Item {
                            checked: f.checked,
                            blocks: f.blocks,
                        });
                    }
                    Kind::Strong => parent.inlines.push(Inline::Strong(frame.inlines)),
                    Kind::Em => parent.inlines.push(Inline::Em(frame.inlines)),
                    Kind::Strike => parent.inlines.push(Inline::Strike(frame.inlines)),
                    Kind::Link(url) => parent.inlines.push(Inline::Link {
                        url,
                        content: frame.inlines,
                    }),
                    Kind::Image(url) => {
                        parent.inlines.push(Inline::Image {
                            url,
                            alt: plain(&frame.inlines),
                        });
                    }
                    Kind::Table(align) => {
                        // The head row is the first collected row.
                        let mut rows = frame.rows.into_iter();
                        let head = rows.next().unwrap_or_default();
                        parent.push_block(Block::Table(Table {
                            align,
                            head,
                            rows: rows.collect(),
                        }));
                    }
                    Kind::Head | Kind::Row => {
                        parent.rows.push(frame.cells);
                    }
                    Kind::Cell => parent.cells.push(frame.inlines),
                    Kind::Doc => {}
                }
            }
            Event::Text(t) => {
                let top = stack.last_mut().expect("document frame");
                match top.kind {
                    Kind::Code(_) | Kind::Html => top.text.push_str(&t),
                    _ => top.inlines.push(Inline::Text(t.into_string())),
                }
            }
            Event::Code(t) => stack
                .last_mut()
                .expect("document frame")
                .inlines
                .push(Inline::Code(t.into_string())),
            Event::Html(h) => {
                let top = stack.last_mut().expect("document frame");
                if matches!(top.kind, Kind::Html) {
                    top.text.push_str(&h);
                } else if !is_comment(&h) {
                    top.inlines.push(Inline::Text(h.into_string()));
                }
            }
            Event::InlineHtml(h) => {
                if !is_comment(&h) {
                    stack
                        .last_mut()
                        .expect("document frame")
                        .inlines
                        .push(Inline::Text(h.into_string()));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                stack
                    .last_mut()
                    .expect("document frame")
                    .inlines
                    .push(Inline::Break);
            }
            Event::Rule => stack
                .last_mut()
                .expect("document frame")
                .push_block(Block::Rule),
            Event::TaskListMarker(done) => {
                // The marker belongs to the item, which is the nearest list item frame.
                if let Some(item) = stack
                    .iter_mut()
                    .rev()
                    .find(|f| matches!(f.kind, Kind::Item))
                {
                    item.checked = Some(done);
                }
            }
            _ => {}
        }
    }
    let mut doc = stack.swap_remove(0);
    doc.flush();
    doc.blocks
}
