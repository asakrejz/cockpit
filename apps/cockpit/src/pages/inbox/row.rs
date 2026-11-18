use super::*;

/// A row of the inbox.
pub(super) fn row(store: Store, item: Item, selected: bool, filter: Filter) -> impl IntoView {
    let id = item.id.clone();
    view! {
        <InboxRow selected=selected on_select=move |_| {
            store.selected.set(id.clone());
            // The entry gets its own address: on narrow screens that opens the detail level.
            navigate(&Route::InboxItem(filter, id.clone()));
        }>
            <UnreadDot read=item.read />
            <span class="ui-vstack" style="--spacing: 4px; flex: 1 1 auto; min-width: 0">
                <span class="ui-hstack" style="--spacing: 6px; flex-wrap: nowrap">
                    <Badge variant=BadgeVariant::Neutral>{item.source.label()}</Badge>
                    <span class="t-footnote t-mono t-secondary">{item.display_key()}</span>
                    <span class="ui-spacer"></span>
                    <span class="t-footnote t-secondary" style="white-space: nowrap">{item.time}</span>
                </span>
                <span class="t-headline ck-ellipsis">{item.title}</span>
                <span class="ui-hstack" style="--spacing: 8px; flex-wrap: nowrap">
                    <span
                        class="ui-list__icon ck-reason"
                        style=format!("background: var(--sys-color-{})", view::tone(item.reason.tone()).id())
                        aria-hidden="true"
                    >
                        <Icon path=view::reason_icon(item.reason) size=13 stroke=2.4 />
                    </span>
                    <span class="t-subheadline ck-ellipsis" style="flex: 1 1 auto">{item.reason_text}</span>
                    <Avatar initials=item.owner.initials title=item.owner.name />
                </span>
            </span>
        </InboxRow>
    }
}

/// Icon at the start of a row in the section "Entwicklung".
pub(super) fn row_icon(path: &'static str) -> impl IntoView {
    view! {
        <span style="display: inline-flex; flex: none; color: var(--sys-color-secondary-label)">
            <Icon path=path />
        </span>
    }
}
