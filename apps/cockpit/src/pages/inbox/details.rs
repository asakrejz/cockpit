//! The fields and attachments of a Jira ticket in the detail pane, and the links to the
//! service next to the ticket and pull request numbers.

use cockpit_core::details::size_text;
use cockpit_core::{Attachment, DetailField, DetailValue, Details, Item};

use super::*;

/// A link that opens `href` in a new tab, with the small "external" icon. The label is
/// `content`; `title` says where it goes (also for screen readers).
pub(super) fn external_link(
    href: String,
    title: String,
    class: &'static str,
    content: AnyView,
) -> AnyView {
    view! {
        <a class=class href=href target="_blank" rel="noopener noreferrer" title=title.clone() aria-label=title>
            {content}
            <span class="ck-external" aria-hidden="true">
                <Icon path=icons::EXTERNAL />
            </span>
        </a>
    }
    .into_any()
}

/// The number of the entry (`BAU-142`, `acme/app#87`) as a link to the service – or plain
/// text if no address is known.
pub(super) fn key_link(item: &Item) -> AnyView {
    let key = item.display_key();
    match item.web_url() {
        Some(url) => {
            let title = t!(
                InboxDetailOpenExternal,
                key = key.clone(),
                source = item.source.label()
            );
            external_link(
                url,
                title,
                "t-subheadline t-mono ck-key-link",
                key.into_any(),
            )
        }
        None => view! { <span class="t-subheadline t-mono t-secondary">{key}</span> }.into_any(),
    }
}

/// The name of a field: the translated one for the standard fields, otherwise Jira's own.
fn field_name(f: &DetailField) -> String {
    match f.id.as_str() {
        "status" => t!(InboxDetailStatus),
        "priority" => t!(InboxDetailPriority),
        "issuetype" => t!(InboxDetailFieldIssuetype),
        "assignee" => t!(InboxDetailFieldAssignee),
        "reporter" => t!(InboxDetailFieldReporter),
        "labels" => t!(InboxDetailFieldLabels),
        "components" => t!(InboxDetailFieldComponents),
        "fixVersions" => t!(InboxDetailFieldFixVersions),
        "sprint" => t!(InboxDetailFieldSprint),
        "story_points" => t!(InboxDetailFieldStoryPoints),
        "duedate" => t!(InboxDetailFieldDuedate),
        "created" => t!(InboxDetailFieldCreated),
        "updated" => t!(InboxDetailFieldUpdated),
        "parent" => t!(InboxDetailFieldParent),
        "issuelinks" => t!(InboxDetailFieldIssuelinks),
        _ if !f.name.is_empty() => f.name.clone(),
        _ => f.id.clone(),
    }
}

fn field_value(v: &DetailValue) -> AnyView {
    match v {
        DetailValue::Text(t) => {
            view! { <span style="white-space: pre-line">{t.clone()}</span> }.into_any()
        }
        DetailValue::Date(d) => view! { <time>{d.clone()}</time> }.into_any(),
        DetailValue::List(items) => {
            let items = items.clone();
            view! {
                <ul class="ck-field__list">
                    {items.into_iter().map(|i| view! { <li><Chip>{i}</Chip></li> }).collect_view()}
                </ul>
            }
            .into_any()
        }
        DetailValue::Rich(doc) => view! { <RichText doc=doc.clone() /> }.into_any(),
    }
}

fn field_row(f: &DetailField) -> impl IntoView {
    view! {
        <li class="ck-field">
            <span class="ck-field__name t-subheadline">{field_name(f)}</span>
            <div class="ck-field__value t-body">{field_value(&f.value)}</div>
        </li>
    }
}

fn attachment_card(a: &Attachment) -> impl IntoView {
    let id = &a.id;
    let size = size_text(a.size);
    let name = a.name.clone();
    let file = format!("/api/jira/attachment/{id}");
    let picture = if a.is_image() {
        // The small picture if Jira made one, otherwise the picture itself.
        let src = if a.thumbnail {
            format!("{file}/thumbnail")
        } else {
            file.clone()
        };
        view! { <img src=src alt=t!(UiImageOf, name = name.clone()) loading="lazy" decoding="async" /> }.into_any()
    } else {
        view! {
            <span class="ck-attachment__none t-secondary">
                <Icon path=icons::IMAGE />
                <span class="t-footnote">{t!(InboxDetailAttachmentNoPreview)}</span>
            </span>
        }
        .into_any()
    };
    let (title, download) = if a.is_image() {
        (t!(InboxDetailAttachmentOpen, name = name.clone()), false)
    } else {
        (t!(InboxDetailAttachmentDownload, name = name.clone()), true)
    };
    let mime = if a.mime.is_empty() {
        String::new()
    } else {
        format!(" · {}", a.mime)
    };
    view! {
        <li class="ck-attachment">
            <a
                class="ck-attachment__thumb"
                href=file
                target="_blank"
                rel="noopener noreferrer"
                title=title.clone()
                aria-label=title
                download=download.then(|| name.clone())
            >
                {picture}
            </a>
            <span class="ck-attachment__name t-subheadline">{name}</span>
            <span class="t-footnote t-secondary">{format!("{size}{mime}")}</span>
        </li>
    }
}

/// The "Details" and "Attachments" sections of a ticket; nothing for entries without them.
pub(super) fn details_sections(details: Option<&Details>) -> AnyView {
    let Some(details) = details.filter(|d| !d.is_empty()) else {
        return ().into_any();
    };
    let fields = (!details.fields.is_empty()).then(|| {
        view! {
            <div class="ui-list__section">
                <p class="ui-list__header">{t!(InboxDetailDetails)}</p>
                <ul class="ck-fields">{details.fields.iter().map(field_row).collect_view()}</ul>
            </div>
        }
    });
    let attachments = (!details.attachments.is_empty()).then(|| {
        view! {
            <div class="ui-list__section">
                <p class="ui-list__header">{t!(InboxDetailAttachments)}</p>
                <ul class="ck-attachments">{details.attachments.iter().map(attachment_card).collect_view()}</ul>
            </div>
        }
    });
    view! { {fields} {attachments} }.into_any()
}
