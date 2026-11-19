use super::parse::safe_link;
use super::*;

fn text(t: &str) -> Inline {
    Inline::Text(t.to_string())
}

#[test]
fn headings_and_paragraphs() {
    let doc = parse("# Title\n\n### Sub\n\nHello **bold**, *em*, ~~gone~~ and `code`.");
    assert_eq!(doc[0], Block::Heading(1, vec![text("Title")]));
    assert_eq!(doc[1], Block::Heading(3, vec![text("Sub")]));
    assert_eq!(
        doc[2],
        Block::Paragraph(vec![
            text("Hello "),
            Inline::Strong(vec![text("bold")]),
            text(", "),
            Inline::Em(vec![text("em")]),
            text(", "),
            Inline::Strike(vec![text("gone")]),
            text(" and "),
            Inline::Code("code".into()),
            text("."),
        ])
    );
}

#[test]
fn lists_ordered_and_nested() {
    let doc = parse("1. one\n2. two\n   - inner\n");
    let Block::List { start, items } = &doc[0] else {
        panic!("list")
    };
    assert_eq!(*start, Some(1));
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].blocks, vec![Block::Paragraph(vec![text("one")])]);
    assert!(matches!(
        items[1].blocks[1],
        Block::List { start: None, .. }
    ));
}

#[test]
fn task_lists_are_checked_or_not() {
    let doc = parse("- [ ] open\n- [x] done\n- plain\n");
    let Block::List { items, .. } = &doc[0] else {
        panic!("list")
    };
    assert_eq!(items[0].checked, Some(false));
    assert_eq!(items[1].checked, Some(true));
    assert_eq!(items[2].checked, None);
    assert_eq!(items[1].blocks, vec![Block::Paragraph(vec![text("done")])]);
}

#[test]
fn fenced_code_keeps_language_and_text() {
    let doc = parse("```rust\nfn main() {}\n```\n");
    assert_eq!(
        doc[0],
        Block::Code {
            language: "rust".into(),
            text: "fn main() {}".into()
        }
    );
}

#[test]
fn quote_rule_and_break() {
    let doc = parse("> quoted\n\n---\n\nline one\nline two");
    assert!(matches!(&doc[0], Block::Quote(b) if b.len() == 1));
    assert_eq!(doc[1], Block::Rule);
    assert_eq!(
        doc[2],
        Block::Paragraph(vec![text("line one"), Inline::Break, text("line two")])
    );
}

#[test]
fn tables_have_head_rows_and_alignment() {
    let doc = parse("| a | b |\n|:--|--:|\n| 1 | 2 |\n| 3 | 4 |\n");
    let Block::Table(t) = &doc[0] else {
        panic!("table")
    };
    assert_eq!(t.head, vec![vec![text("a")], vec![text("b")]]);
    assert_eq!(t.rows.len(), 2);
    assert_eq!(t.rows[1], vec![vec![text("3")], vec![text("4")]]);
    assert_eq!(t.align, vec![Align::Left, Align::Right]);
}

#[test]
fn allowed_links_are_kept() {
    let doc = parse(
        "[a](https://example.com) [b](http://example.com) [c](mailto:x@example.com) <https://example.org>",
    );
    let Block::Paragraph(p) = &doc[0] else {
        panic!("paragraph")
    };
    let urls: Vec<_> = p
        .iter()
        .filter_map(|i| match i {
            Inline::Link { url, .. } => Some(url.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        urls,
        vec![
            Some("https://example.com".to_string()),
            Some("http://example.com".to_string()),
            Some("mailto:x@example.com".to_string()),
            Some("https://example.org".to_string()),
        ]
    );
}

#[test]
fn forbidden_schemes_lose_the_link_but_keep_the_text() {
    for url in [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        "data:text/html;base64,AAAA",
        "vbscript:x",
        "file:///etc/passwd",
        "/relative",
        "//evil.example",
    ] {
        let doc = parse(&format!("[click]({url})"));
        assert_eq!(
            doc[0],
            Block::Paragraph(vec![Inline::Link {
                url: None,
                content: vec![text("click")]
            }]),
            "{url}"
        );
    }
    assert_eq!(safe_link("  https://a.b "), Some("https://a.b".to_string()));
    assert_eq!(safe_link("java\nscript:alert(1)"), None);
}

#[test]
fn images_become_links_without_loading() {
    let doc = parse("![shot](https://example.com/a.png) ![bad](javascript:x)");
    let Block::Paragraph(p) = &doc[0] else {
        panic!("paragraph")
    };
    assert_eq!(
        p[0],
        Inline::Image {
            url: Some("https://example.com/a.png".into()),
            alt: "shot".into()
        }
    );
    assert_eq!(
        p[2],
        Inline::Image {
            url: None,
            alt: "bad".into()
        }
    );
}

#[test]
fn raw_html_is_text_never_html() {
    let doc = parse("<script>alert(1)</script>\n\nhi <b>there</b> <img src=x onerror=y>");
    // Block HTML: a code block with the literal source.
    assert_eq!(
        doc[0],
        Block::Code {
            language: String::new(),
            text: "<script>alert(1)</script>".into()
        }
    );
    // Inline HTML: literal text pieces, no element.
    let Block::Paragraph(p) = &doc[1] else {
        panic!("paragraph")
    };
    assert!(p.iter().all(|i| matches!(i, Inline::Text(_))));
    let joined: String = p
        .iter()
        .map(|i| {
            if let Inline::Text(t) = i {
                t.as_str()
            } else {
                ""
            }
        })
        .collect();
    assert_eq!(joined, "hi <b>there</b> <img src=x onerror=y>");
}

#[test]
fn html_comments_are_dropped() {
    let doc = parse("<!-- template hint\nmultiple lines -->\n\ntext <!-- inline --> more");
    assert_eq!(doc.len(), 1);
    let Block::Paragraph(p) = &doc[0] else {
        panic!("paragraph")
    };
    assert!(!format!("{p:?}").contains("template") && !format!("{p:?}").contains("inline"));
}

#[test]
fn empty_text_has_no_blocks() {
    assert!(parse("").is_empty());
    assert!(parse("   \n\n").is_empty());
}
