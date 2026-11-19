//! Formatted Jira text (ADF) as real elements.
//!
//! The tree comes from `cockpit_core::adf`. Everything is built from Leptos elements; text is
//! always set as text, never as HTML. Links are checked again here (`http`/`https` only) and
//! open in a new tab with `rel="noopener noreferrer"`. Media show the picture when an attachment belongs to them (`/api/jira/attachment/{id}`), otherwise only their file name.

use cockpit_core::adf::{Block, Cell, Doc, Inline, Marks, PanelKind, safe_url};
use leptos::prelude::*;

/// Formatted text of a Jira description or comment.
#[component]
pub fn RichText(doc: Doc) -> impl IntoView {
    view! { <div class="ui-rich t-body">{blocks(&doc)}</div> }
}

fn blocks(blocks: &[Block]) -> Vec<AnyView> {
    blocks.iter().map(block).collect()
}

fn block(b: &Block) -> AnyView {
    match b {
        Block::Paragraph { content } => view! { <p>{inlines(content)}</p> }.into_any(),
        Block::Heading { level, content } => heading(*level, inlines(content)),
        Block::BulletList { items } => {
            view! { <ul>{items.iter().map(|i| view! { <li>{blocks(i)}</li> }).collect_view()}</ul> }
                .into_any()
        }
        Block::OrderedList { start, items } => view! {
            <ol start=start.to_string()>
                {items.iter().map(|i| view! { <li>{blocks(i)}</li> }).collect_view()}
            </ol>
        }
        .into_any(),
        Block::CodeBlock { language, text } => view! {
            <pre class="ui-rich__code" data-language=language.clone()>
                <code>{text.clone()}</code>
            </pre>
        }
        .into_any(),
        Block::Quote { content } => view! { <blockquote>{blocks(content)}</blockquote> }.into_any(),
        Block::Rule => view! { <hr /> }.into_any(),
        Block::Table { rows } => view! {
            <div class="ui-rich__table">
                <table>
                    <tbody>
                        {rows
                            .iter()
                            .map(|row| view! { <tr>{row.iter().map(cell).collect_view()}</tr> })
                            .collect_view()}
                    </tbody>
                </table>
            </div>
        }
        .into_any(),
        Block::Panel { kind, content } => view! {
            <div class="ui-rich__panel" data-kind=panel_kind(*kind)>{blocks(content)}</div>
        }
        .into_any(),
        Block::Media {
            name,
            attachment: Some(id),
        } => {
            let src = format!("/api/jira/attachment/{id}");
            let href = src.clone();
            view! {
                <p class="ui-rich__media ui-rich__image">
                    <a href=href target="_blank" rel="noopener noreferrer">
                        <img
                            src=src
                            alt=crate::t!(UiImageOf, name = name)
                            loading="lazy"
                            decoding="async"
                        />
                    </a>
                </p>
            }
            .into_any()
        }
        Block::Media { name, .. } => {
            view! { <p class="ui-rich__media">{crate::t!(UiAttachment, name = name)}</p> }
                .into_any()
        }
    }
}

fn panel_kind(kind: PanelKind) -> &'static str {
    match kind {
        PanelKind::Info => "info",
        PanelKind::Note => "note",
        PanelKind::Success => "success",
        PanelKind::Warning => "warning",
        PanelKind::Error => "error",
    }
}

/// Headings sit below the page and pane titles: levels 1–6 become `h3`–`h6`.
fn heading(level: u8, content: Vec<AnyView>) -> AnyView {
    match level {
        1 => view! { <h3 class="t-title3 t-bold">{content}</h3> }.into_any(),
        2 => view! { <h4 class="t-headline">{content}</h4> }.into_any(),
        3 => view! { <h5 class="t-headline">{content}</h5> }.into_any(),
        _ => view! { <h6 class="t-subheadline t-bold">{content}</h6> }.into_any(),
    }
}

fn cell(c: &Cell) -> AnyView {
    if c.header {
        view! { <th>{blocks(&c.content)}</th> }.into_any()
    } else {
        view! { <td>{blocks(&c.content)}</td> }.into_any()
    }
}

fn inlines(content: &[Inline]) -> Vec<AnyView> {
    content.iter().map(inline).collect()
}

fn inline(i: &Inline) -> AnyView {
    match i {
        Inline::Text { text, marks } => marked(text.clone(), marks),
        Inline::HardBreak => view! { <br /> }.into_any(),
        Inline::Mention { text } => {
            view! { <span class="ui-rich__mention">{text.clone()}</span> }.into_any()
        }
        Inline::Emoji { text } => text.clone().into_any(),
        Inline::InlineCard { url } => link(url, url.clone().into_any()),
        Inline::Status { text, color } => view! {
            <span class="ui-rich__status" data-color=color.clone()>{text.clone()}</span>
        }
        .into_any(),
        Inline::Date { text } => {
            view! { <time class="ui-rich__date">{text.clone()}</time> }.into_any()
        }
    }
}

/// A link to `url` – or just the content if the address is not `http`/`https`.
fn link(url: &str, content: AnyView) -> AnyView {
    match safe_url(url) {
        Some(url) => view! {
            <a href=url.to_string() target="_blank" rel="noopener noreferrer">{content}</a>
        }
        .into_any(),
        None => content,
    }
}

fn marked(text: String, marks: &Marks) -> AnyView {
    let mut out: AnyView = text.into_any();
    if marks.code {
        out = view! { <code>{out}</code> }.into_any();
    }
    if marks.strong {
        out = view! { <strong>{out}</strong> }.into_any();
    }
    if marks.em {
        out = view! { <em>{out}</em> }.into_any();
    }
    if marks.strike {
        out = view! { <s>{out}</s> }.into_any();
    }
    if marks.underline {
        out = view! { <u>{out}</u> }.into_any();
    }
    match &marks.link {
        Some(url) => link(url, out),
        None => out,
    }
}
