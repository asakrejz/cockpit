//! GitHub-flavored Markdown (PR descriptions, review and comment bodies) as real elements.
//!
//! `parse` turns the text into a small tree (see there for what is dropped or limited); this
//! file builds Leptos elements from it. Text is always set as text, never as HTML, and the
//! look is shared with the Jira text (`.ui-rich`). Links open in a new tab with
//! `rel="noopener noreferrer"`. Images are never loaded: they are shown as a link to their address.

mod parse;

#[cfg(test)]
mod tests;

use leptos::prelude::*;

use parse::{Align, Block, Inline, Item, Table, parse};

/// Formatted Markdown text. `class` is the text style of the whole block (default `t-body`).
#[component]
pub fn Markdown(
    #[prop(into)] text: String,
    #[prop(optional, into)] class: Option<String>,
) -> impl IntoView {
    let class = format!(
        "ui-rich ui-md {}",
        class.unwrap_or_else(|| "t-body".to_string())
    );
    view! { <div class=class>{blocks(&parse(&text))}</div> }
}

fn blocks(blocks: &[Block]) -> Vec<AnyView> {
    blocks.iter().map(block).collect()
}

fn block(b: &Block) -> AnyView {
    match b {
        Block::Paragraph(content) => view! { <p>{inlines(content)}</p> }.into_any(),
        Block::Heading(level, content) => crate::rich::heading(*level, inlines(content)),
        Block::Code { language, text } => view! {
            <pre class="ui-rich__code" data-language=(!language.is_empty()).then(|| language.clone())>
                <code>{text.clone()}</code>
            </pre>
        }
        .into_any(),
        Block::Quote(content) => view! { <blockquote>{blocks(content)}</blockquote> }.into_any(),
        Block::Rule => view! { <hr /> }.into_any(),
        Block::List { start, items } => {
            let items = items.iter().map(item).collect_view();
            match start {
                Some(n) => view! { <ol start=n.to_string()>{items}</ol> }.into_any(),
                None => view! { <ul>{items}</ul> }.into_any(),
            }
        }
        Block::Table(t) => table(t),
    }
}

fn item(i: &Item) -> AnyView {
    match i.checked {
        Some(done) => view! {
            <li class="ui-md__task">
                <input type="checkbox" checked=done disabled=true aria-checked=done.to_string() />
                <div>{blocks(&i.blocks)}</div>
            </li>
        }
        .into_any(),
        None => view! { <li>{blocks(&i.blocks)}</li> }.into_any(),
    }
}

fn align_attr(t: &Table, column: usize) -> Option<&'static str> {
    match t.align.get(column) {
        Some(Align::Left) => Some("left"),
        Some(Align::Center) => Some("center"),
        Some(Align::Right) => Some("right"),
        _ => None,
    }
}

fn table(t: &Table) -> AnyView {
    let head = t
        .head
        .iter()
        .enumerate()
        .map(|(c, cell)| view! { <th data-align=align_attr(t, c)>{inlines(cell)}</th> })
        .collect_view();
    let rows = t
        .rows
        .iter()
        .map(|row| {
            let cells = row
                .iter()
                .enumerate()
                .map(|(c, cell)| view! { <td data-align=align_attr(t, c)>{inlines(cell)}</td> })
                .collect_view();
            view! { <tr>{cells}</tr> }
        })
        .collect_view();
    view! {
        <div class="ui-rich__table">
            <table>
                <thead><tr>{head}</tr></thead>
                <tbody>{rows}</tbody>
            </table>
        </div>
    }
    .into_any()
}

fn inlines(content: &[Inline]) -> Vec<AnyView> {
    content.iter().map(inline).collect()
}

fn inline(i: &Inline) -> AnyView {
    match i {
        Inline::Text(t) => t.clone().into_any(),
        Inline::Code(t) => view! { <code>{t.clone()}</code> }.into_any(),
        Inline::Strong(c) => view! { <strong>{inlines(c)}</strong> }.into_any(),
        Inline::Em(c) => view! { <em>{inlines(c)}</em> }.into_any(),
        Inline::Strike(c) => view! { <s>{inlines(c)}</s> }.into_any(),
        Inline::Break => view! { <br /> }.into_any(),
        Inline::Link { url, content } => link(url.as_deref(), inlines(content).into_any()),
        Inline::Image { url, alt } => {
            let label = match (alt.is_empty(), url) {
                (false, _) => crate::t!(UiMarkdownImage, alt = alt),
                (true, Some(u)) => crate::t!(UiMarkdownImage, alt = u),
                (true, None) => crate::t!(UiMarkdownImage, alt = ""),
            };
            link(url.as_deref(), label.into_any())
        }
    }
}

/// A link to `url` – or just the content if there is no allowed address.
fn link(url: Option<&str>, content: AnyView) -> AnyView {
    match url {
        Some(url) => view! {
            <a href=url.to_string() target="_blank" rel="noopener noreferrer">{content}</a>
        }
        .into_any(),
        None => content,
    }
}
