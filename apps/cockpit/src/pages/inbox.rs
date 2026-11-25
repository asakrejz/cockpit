//! Attention: on the left the inbox of tickets and pull requests, on the right the detail
//! with status, branch, comments and the way to the agent.

use cockpit_core::{
    Action, Filter, InboxFilter, Item, Person, Priority, Rule, SnoozeChoice, SourceFilter,
    TicketStatus,
};
use feature_ui::*;
use leptos::{ev, prelude::*, wasm_bindgen::JsCast};
use web_sys::{HtmlElement, KeyboardEvent, ScrollIntoViewOptions, ScrollLogicalPosition};

use crate::{
    access_hint::{AccessBanner, AccessHint, problem_of, use_github_access},
    route::Route,
    server::{self, CallError},
    store::{Store, navigate, now_ms, use_store, utc_offset_min},
    sync_status::sync_now,
    view,
};

#[component]
pub fn InboxPage(filter: Filter) -> impl IntoView {
    let store = use_store();

    // The memos only fire when their result really changes. A ticking
    // agent on another ticket therefore does not rebuild the detail.
    // The search text belongs only to this page; it does not persist across navigation.
    let query = RwSignal::new(String::new());
    // The filters (definitions: `cockpit_core::FILTERS`) are part of the workspace and change
    // in the filter dialog by `Action::SetInboxFilter`.
    let inbox_filter = Memo::new(move |_| store.ws.with(|w| w.inbox_filter.clone()));
    let filter_open = RwSignal::new(false);
    let items = Memo::new(move |_| {
        let (query, now) = (query.get(), store.now.get());
        let inbox_filter = inbox_filter.get();
        store.ws.with(|w| {
            w.inbox(filter, SourceFilter::All, &query, now, &inbox_filter)
                .into_iter()
                .map(|item| with_reason(w, item, filter, now))
                .collect::<Vec<Item>>()
        })
    });
    // An entry opened by its address (`#/item/<id>`) that the filters hide: it is shown anyway,
    // with a note and a way to reset the filters.
    let hidden = Memo::new(move |_| {
        let Route::InboxItem(_, id) = store.route.get() else {
            return None;
        };
        let (query, now) = (query.get(), store.now.get());
        let by = inbox_filter.get();
        store.ws.with(|w| {
            w.hidden_by_filter(&id, filter, &query, now, &by)
                .then(|| with_reason(w, w.item(&id).expect("hidden entry exists"), filter, now))
        })
    });
    // The chosen entry. If it is not visible in the filter, the first one applies – unless it
    // was opened by address.
    let current = Memo::new(move |_| {
        if let Some(item) = hidden.get() {
            return Some(item);
        }
        let id = store.selected.get();
        let list = items.get();
        list.iter().find(|i| i.id == id).or(list.first()).cloned()
    });

    // Draft of the reply. With every change of the entry a new one begins.
    let draft = RwSignal::new(String::new());
    Effect::new(move |_| {
        store.selected.track();
        draft.set(String::new());
    });

    // Narrow screens show list and detail as two levels; the address says which one is on.
    // An address with an entry also chooses that entry.
    let level = Signal::derive(move || store.route.with(Route::level));
    Effect::new(move |_| {
        if let Route::InboxItem(_, id) = store.route.get() {
            store.selected.set(id);
        }
    });
    let was_detail = StoredValue::new(false);
    Effect::new(move |_| {
        let detail = level.get() == StackLevel::Detail;
        let before = was_detail.get_value();
        was_detail.set_value(detail);
        if detail != before {
            request_animation_frame(move || {
                if detail {
                    window().scroll_to_with_x_and_y(0.0, 0.0);
                } else if let Some(row) = query_html(".ck-inbox [aria-pressed=\"true\"]") {
                    let options = ScrollIntoViewOptions::new();
                    options.set_block(ScrollLogicalPosition::Center);
                    row.scroll_into_view_with_scroll_into_view_options(&options);
                }
            });
        }
    });

    // Edit mode of a ticket: draft of title and description. With every change
    // of the entry it ends; a draft does not carry over to another ticket.
    let edit = Edit {
        on: RwSignal::new(false),
        title: RwSignal::new(String::new()),
        description: RwSignal::new(String::new()),
    };
    Effect::new(move |_| {
        store.selected.track();
        edit.on.set(false);
    });

    // Keyboard control: applies as long as the page is open and ends with it.
    let listener = window_event_listener(ev::keydown, move |event| {
        on_key(store, items, current, event)
    });
    on_cleanup(move || listener.remove());

    // The chosen entry should stay visible, even if it changes by keyboard.
    Effect::new(move |_| {
        current.track();
        request_animation_frame(|| {
            if let Some(row) = query_html(".ck-inbox [aria-pressed=\"true\"]") {
                let options = ScrollIntoViewOptions::new();
                options.set_block(ScrollLogicalPosition::Nearest);
                row.scroll_into_view_with_scroll_into_view_options(&options);
            }
        });
    });

    let syncing = RwSignal::new(false);
    let (access, recheck) = use_github_access();
    let (title, empty) = match filter {
        Filter::All => (t!(InboxTitleAll), t!(InboxEmptyAll)),
        Filter::Mine => (t!(InboxTitleMine), t!(InboxEmptyMine)),
        Filter::Team => (t!(InboxTitleTeam), t!(InboxEmptyTeam)),
        Filter::Snoozed => (t!(InboxTitleSnoozed), t!(InboxEmptySnoozed)),
    };
    view! {
        <SplitView level=level back_label=title.clone() back_href=Route::Inbox(filter).hash()>
            <Pane label=title.clone()>
                <PaneHeader>
                    <div class="ui-hstack ui-justify-between ui-flex-nowrap">
                        <h1 class="t-large-title t-bold">{title}</h1>
                        <Badge variant=BadgeVariant::Count>
                            {move || {
                                let now = store.now.get();
                                store.ws.with(|w| w.unread(filter, now)).to_string()
                            }}
                        </Badge>
                    </div>
                    <Picker
                        options=labels::options(Filter::IDS, Filter::from_id, labels::filter_tab)
                        value=filter.id()
                        on_change=move |id: &'static str| {
                            if let Some(filter) = Filter::from_id(id) {
                                crate::last_filter::set(filter);
                                navigate(&Route::Inbox(filter));
                            }
                        }
                        aria_label=t!(InboxTabsLabel)
                    />
                    <div class="ck-filters">
                        {filter_button(store, filter_open)} <project::NewIssue store=store />
                    </div>
                    {filter_chips(store)}
                    <div class="ck-search">
                        <SearchField value=query placeholder=t!(InboxSearch) />
                    </div>
                    <p class="ui-sr-only" role="status">
                        {move || t!(InboxEntries, n = items.with(Vec::len))}
                    </p>
                </PaneHeader>
                {move || {
                    problem_of(&access)
                        .filter(|_| store.ws.with(|w| !w.items.is_empty()))
                        .map(|a| view! { <AccessBanner access=a /> })
                }}
                <ul class="ck-inbox">
                    {move || {
                        let list = items.get();
                        if list.is_empty() && is_narrowed(&query.get(), &inbox_filter.get()) {
                            let reset = move |_| {
                                query.set(String::new());
                                let _ = store
                                    .dispatch(Action::SetInboxFilter {
                                        filter: InboxFilter::default(),
                                    });
                            };
                            view! {
                                <li class="ck-empty">
                                    <EmptyState
                                        title=t!(InboxNoMatchTitle)
                                        text=t!(InboxNoMatchText)
                                        icon=icons::SEARCH
                                    >
                                        <Button size=ControlSize::Small on_click=reset>
                                            {t!(InboxResetFilters)}
                                        </Button>
                                    </EmptyState>
                                </li>
                            }
                                .into_any()
                        } else if list.is_empty() && store.ws.with(|w| w.items.is_empty()) {
                            // A fresh workspace: nothing connected or synced yet.
                            let connected = store.jira_connected() || store.github_connected();
                            if let Some(a) = problem_of(&access) {
                                view! {
                                    <li class="ck-empty">
                                        <AccessHint access=a recheck=recheck />
                                    </li>
                                }
                                    .into_any()
                            } else if connected {
                                view! {
                                    <li class="ck-empty">
                                        <EmptyState
                                            title=t!(InboxFreshTitle)
                                            text=t!(InboxFreshText)
                                            icon=icons::TASKS
                                        >
                                            <Button
                                                size=ControlSize::Small
                                                variant=ButtonStyle::GlassProminent
                                                disabled=Signal::derive(move || syncing.get())
                                                on_click=move |_| sync_now(store, syncing)
                                            >
                                                {t!(InboxSyncNow)}
                                            </Button>
                                            <Button size=ControlSize::Small href=Route::Setup.hash()>
                                                {t!(InboxSetup)}
                                            </Button>
                                        </EmptyState>
                                    </li>
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <li class="ck-empty">
                                        <EmptyState
                                            title=t!(InboxUnconnectedTitle)
                                            text=t!(InboxUnconnectedText)
                                            icon=icons::TASKS
                                        >
                                            <Button
                                                size=ControlSize::Small
                                                variant=ButtonStyle::GlassProminent
                                                href=Route::Setup.hash()
                                            >
                                                {t!(InboxSetup)}
                                            </Button>
                                        </EmptyState>
                                    </li>
                                }
                                    .into_any()
                            }
                        } else if list.is_empty() {
                            let hint = if matches!(filter, Filter::Snoozed) {
                                t!(InboxHintSnoozed)
                            } else {
                                t!(InboxHintNew)
                            };
                            view! {
                                <li class="ck-empty">
                                    <EmptyState title=empty.clone() text=hint icon=icons::CHECK />
                                </li>
                            }
                                .into_any()
                        } else {
                            let active = current.get().map(|i| i.id);
                            list.into_iter()
                                .map(|item| {
                                    let selected = active.as_deref() == Some(item.id.as_str());
                                    row(store, item, selected, filter)
                                })
                                .collect_view()
                                .into_any()
                        }
                    }}
                </ul>
                <p class="ck-keys t-footnote t-secondary">
                    <span><kbd>"↑"</kbd><kbd>"↓"</kbd>{format!(" {}", t!(InboxKeySelect))}</span>
                    <span><kbd>"e"</kbd>{format!(" {}", t!(InboxKeyDone))}</span>
                    <span><kbd>"r"</kbd>{format!(" {}", t!(InboxKeyReply))}</span>
                    <span><kbd>"s"</kbd>{format!(" {}", t!(InboxKeySnooze))}</span>
                    <span><kbd>"u"</kbd>{format!(" {}", t!(InboxKeyUnsnooze))}</span>
                    <span><kbd>"/"</kbd>{format!(" {}", t!(InboxKeySearch))}</span>
                </p>
            </Pane>

            <Pane label=t!(InboxDetails)>
                {move || match current.get() {
                    Some(item) => {
                        let id = item.id.clone();
                        let reset = move |_| {
                            query.set(String::new());
                            let now = store.now.get_untracked();
                            let filter = store.ws.with_untracked(|w| w.filters_revealing(&id, filter, now));
                            let _ = store.dispatch(Action::SetInboxFilter { filter });
                        };
                        view! {
                            {hidden
                                .get()
                                .is_some()
                                .then(|| hidden_note(reset))}
                            {detail(store, item, items, draft, edit)}
                        }
                            .into_any()
                    }
                    None => {
                        view! {
                            <PaneBody>
                                <EmptyState
                                    title=t!(InboxNoneSelectedTitle)
                                    text=t!(InboxNoneSelectedText)
                                />
                            </PaneBody>
                        }
                            .into_any()
                    }
                }}
            </Pane>
        </SplitView>
        {filter_dialog(store, filter_open)}
    }
}

/// „Durch Filter ausgeblendet“ with the button that resets the filters.
fn hidden_note(reset: impl Fn(ev::MouseEvent) + Send + Sync + 'static) -> impl IntoView {
    view! {
        <div class="ck-hidden-note" role="status">
            <p class="t-headline">{t!(InboxHiddenByFilter)}</p>
            <p class="t-subheadline t-secondary">{t!(InboxHiddenByFilterText)}</p>
            <Button size=ControlSize::Small on_click=reset>
                {t!(InboxResetFilters)}
            </Button>
        </div>
    }
}

/// The entry with the reason the rules deliver: the most important hit determines icon,
/// color and sentence in the row. In "Zurückgestellt" the time until when is shown instead.
fn with_reason(w: &cockpit_core::Workspace, item: &Item, filter: Filter, now: u64) -> Item {
    let mut item = item.clone();
    if let Some(hit) = w.hits(&item, now).into_iter().next() {
        item.reason = hit.reason;
        item.reason_msg = Some(hit.text);
        item.reason_text.clear();
    }
    if filter == Filter::Snoozed
        && let Some(snooze) = w.snooze_of(&item.id)
    {
        item.reason_msg = Some(snooze_msg(snooze.until_ms));
        item.reason_text.clear();
    }
    item
}

/// "Zurückgestellt bis Do., 12.11., 08:00" or "… bis sich etwas ändert".
fn snooze_msg(until_ms: Option<u64>) -> cockpit_i18n::Msg {
    match until_ms {
        Some(ms) => cockpit_i18n::msg!(InboxSnoozedUntil, time = format_time(ms)),
        None => cockpit_i18n::msg!(InboxSnoozedUntilChange),
    }
}

fn snooze_text(until_ms: Option<u64>) -> String {
    snooze_msg(until_ms).render(lang())
}

/// Time in the browser's local time, short and in the language of the interface.
fn format_time(ms: u64) -> String {
    let options = js_sys::Object::new();
    for (key, value) in [
        ("weekday", "short"),
        ("day", "numeric"),
        ("month", "numeric"),
        ("hour", "2-digit"),
        ("minute", "2-digit"),
    ] {
        let _ = js_sys::Reflect::set(&options, &key.into(), &value.into());
    }
    js_sys::Date::new(&(ms as f64).into())
        .to_locale_string(lang().code(), &options)
        .as_string()
        .unwrap_or_default()
}

/// Draft in the edit mode of a ticket.
#[derive(Clone, Copy)]
struct Edit {
    on: RwSignal<bool>,
    title: RwSignal<String>,
    description: RwSignal<String>,
}

/// Snoozes an entry and then selects the neighbor in the list.
fn snooze(store: Store, items: Memo<Vec<Item>>, id: &str, choice: SnoozeChoice) {
    let until_ms = choice.until_ms(now_ms(), utc_offset_min());
    let list = items.get_untracked();
    let neighbour = list.iter().position(|i| i.id == id).and_then(|at| {
        list.get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|n| list.get(n)))
            .map(|i| i.id.clone())
    });
    if store
        .dispatch(Action::Snooze {
            item: id.to_string(),
            until_ms,
        })
        .is_ok()
        && let Some(next) = neighbour
    {
        store.selected.set(next);
    }
}

mod assignee;
mod controls;
mod detail;
mod details;
mod filters;
mod keyboard;
pub mod labels;
pub mod picker;
mod project;
mod row;

use controls::is_narrowed;
use detail::detail;
use filters::{filter_button, filter_chips, filter_dialog};
use keyboard::{on_key, query_html};
use picker::Picker;
use row::{row, row_icon};
