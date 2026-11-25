use super::*;

/// A row of the inbox.
pub(super) fn row(store: Store, item: Item, selected: bool, filter: Filter) -> impl IntoView {
    let id = item.id.clone();
    let reason = item.reason_line();
    let time_of = item.clone();
    view! {
        <InboxRow selected=selected on_select=move |_| {
            store.selected.set(id.clone());
            // The entry gets its own address: on narrow screens that opens the detail level.
            navigate(&Route::InboxItem(filter, id.clone()));
        }>
            <UnreadDot read=item.read />
            <span class="ui-vstack ui-gap-4 ui-grow">
                <span class="ui-hstack ui-gap-6 ui-flex-nowrap">
                    <Badge variant=BadgeVariant::Neutral>{item.source.label()}</Badge>
                    <span class="t-footnote t-mono t-secondary">{item.display_key()}</span>
                    <span class="ui-spacer"></span>
                    <span class="t-footnote t-secondary ui-text-nowrap">
                        {move || labels::item_time(&time_of, store.now.get())}
                    </span>
                </span>
                <span class="t-headline ck-ellipsis">{item.title}</span>
                <span class="ui-hstack ui-gap-8 ui-flex-nowrap">
                    <span
                        class="ui-list__icon ck-reason"
                        style=format!("background: var(--sys-color-{})", view::tone(item.reason.tone()).id())
                        aria-hidden="true"
                    >
                        <Icon path=view::reason_icon(item.reason) size=13 stroke=2.4 />
                    </span>
                    <span class="t-subheadline ck-ellipsis ui-grow">{move || reason.render(lang())}</span>
                    <Avatar initials=item.owner.initials_in(lang()) title=item.owner.name_in(lang()) />
                </span>
            </span>
        </InboxRow>
    }
}

/// Icon at the start of a row in the section "Entwicklung".
pub(super) fn row_icon(path: &'static str) -> impl IntoView {
    view! {
        <span class="ui-icon-muted ui-flex-none">
            <Icon path=path />
        </span>
    }
}
