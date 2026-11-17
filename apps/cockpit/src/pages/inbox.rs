//! Attention: on the left the inbox of tickets and pull requests, on the right the detail
//! with status, branch, comments and the way to the agent.

use cockpit_core::{
    Action, Applies, Choices, FILTERS, Filter, FilterDef, InboxFilter, Item, Person, Priority,
    Rule, SnoozeChoice, SourceFilter, TicketStatus,
};
use feature_ui::*;
use leptos::{ev, prelude::*, wasm_bindgen::JsCast};
use web_sys::{HtmlElement, KeyboardEvent, ScrollIntoViewOptions, ScrollLogicalPosition};

use crate::{
    route::Route,
    server::{self, CallError},
    sidebar::MoreSection,
    store::{Store, navigate, now_ms, use_store, utc_offset_min},
    sync_status::sync_now,
    view,
};

#[component]
pub fn InboxPage(filter: Filter) -> impl IntoView {
    let store = use_store();

    // The memos only fire when their result really changes. A ticking
    // agent on another ticket therefore does not rebuild the detail.
    // Search text and source belong only to this page; they do not persist across navigation.
    let query = RwSignal::new(String::new());
    let source = RwSignal::new(SourceFilter::All);
    // The other filters (definitions: `cockpit_core::FILTERS`) live together in one value that
    // starts at the defaults: own entries, active sprint, done tickets hidden, rest „all“.
    let by = RwSignal::new(InboxFilter::default());
    let inbox_filter = Memo::new(move |_| by.get());
    let items = Memo::new(move |_| {
        let (source, query, now) = (source.get(), query.get(), store.now.get());
        let inbox_filter = inbox_filter.get();
        store.ws.with(|w| {
            w.inbox(filter, source, &query, now, &inbox_filter)
                .into_iter()
                .map(|item| with_reason(w, item, filter, now))
                .collect::<Vec<Item>>()
        })
    });
    // The chosen entry. If it is not visible in the filter, the first one applies.
    let current = Memo::new(move |_| {
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

    // Narrow screens show list and detail as two levels; this says which one is on.
    let detail_open = RwSignal::new(false);
    Effect::new(move |previous: Option<bool>| {
        let open = detail_open.get();
        if previous.is_some() {
            request_animation_frame(move || {
                if open {
                    window().scroll_to_with_x_and_y(0.0, 0.0);
                } else if let Some(row) = query_html(".ck-inbox [aria-pressed=\"true\"]") {
                    let options = ScrollIntoViewOptions::new();
                    options.set_block(ScrollLogicalPosition::Center);
                    row.scroll_into_view_with_scroll_into_view_options(&options);
                }
            });
        }
        open
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
    let (title, empty) = match filter {
        Filter::All => ("Aufmerksamkeit", "Nichts wartet auf dich."),
        Filter::Mine => ("Meine Aufgaben", "Du hast gerade keine Aufgaben."),
        Filter::Team => ("Team", "Im Team ist gerade nichts offen."),
        Filter::Snoozed => ("Zurückgestellt", "Nichts ist zurückgestellt."),
    };
    view! {
        <SplitView detail_open=detail_open>
            // The rules live in the "Mehr" sheet of the navigation.
            <MoreSection>
                <p class="ui-sidebar__label">"Regeln"</p>
                <ul class="ui-sidebar__nav">
                    {Rule::ALL.into_iter().map(|rule| rule_switch(store, rule)).collect_view()}
                </ul>
                <p class="ui-sidebar__label">"Tickets"</p>
                <ul class="ui-sidebar__nav">{status_follow_switch(store)}</ul>
            </MoreSection>

            <Pane label=title>
                <PaneHeader>
                    <div class="ui-hstack" style="justify-content: space-between; flex-wrap: nowrap">
                        <h1 class="t-large-title t-bold">{title}</h1>
                        <Badge variant=BadgeVariant::Count>
                            {move || {
                                let now = store.now.get();
                                store.ws.with(|w| w.unread(filter, now)).to_string()
                            }}
                        </Badge>
                    </div>
                    <SegmentedPicker
                        options=Filter::OPTIONS
                        value=filter.id()
                        on_change=move |id: &'static str| {
                            if let Some(filter) = Filter::from_id(id) {
                                navigate(&Route::Inbox(filter));
                            }
                        }
                        aria_label="Filter"
                    />
                    <SegmentedPicker
                        options=SourceFilter::OPTIONS
                        value=Signal::derive(move || source.get().id())
                        on_change=move |id: &'static str| {
                            source.set(SourceFilter::from_id(id).unwrap_or_default());
                        }
                        aria_label="Quelle"
                    />
                    {project_picker(store)}
                    <div class="ck-filters">
                        {filter_bar(store, by)}
                        <Show when=move || !by.with(InboxFilter::is_default)>
                            <Button
                                size=ControlSize::Small
                                on_click=move |_| by.set(InboxFilter::default())
                            >
                                "Filter zurücksetzen"
                            </Button>
                        </Show>
                    </div>
                    <div class="ck-search">
                        <SearchField value=query placeholder="Inbox durchsuchen" />
                    </div>
                    <p class="ui-sr-only" role="status">
                        {move || format!("{} Einträge", items.with(Vec::len))}
                    </p>
                </PaneHeader>
                <ul class="ck-inbox">
                    {move || {
                        let list = items.get();
                        if list.is_empty() && is_narrowed(&query.get(), source.get(), &inbox_filter.get()) {
                            let reset = move |_| {
                                query.set(String::new());
                                source.set(SourceFilter::All);
                                by.set(InboxFilter::default());
                            };
                            view! {
                                <li class="ck-empty">
                                    <EmptyState
                                        title="Keine Treffer"
                                        text="Zu Suche und Filtern passt kein Eintrag."
                                        icon=icons::SEARCH
                                    >
                                        <Button size=ControlSize::Small on_click=reset>
                                            "Filter zurücksetzen"
                                        </Button>
                                    </EmptyState>
                                </li>
                            }
                                .into_any()
                        } else if list.is_empty() && store.ws.with(|w| w.items.is_empty()) {
                            // A fresh workspace: nothing connected or synced yet.
                            let connected = store.conn.with(|c| c.jira || c.github);
                            if connected {
                                view! {
                                    <li class="ck-empty">
                                        <EmptyState
                                            title="Noch keine Tickets oder Pull Requests"
                                            text="Jira oder GitHub ist verbunden, hat aber noch nichts geliefert. Gleiche jetzt ab oder prüfe unter „Einrichten“, welches Projekt und welche Repos gelten."
                                            icon=icons::TASKS
                                        >
                                            <Button
                                                size=ControlSize::Small
                                                variant=ButtonStyle::GlassProminent
                                                disabled=Signal::derive(move || syncing.get())
                                                on_click=move |_| sync_now(store, syncing)
                                            >
                                                "Jetzt abgleichen"
                                            </Button>
                                            <Button size=ControlSize::Small href=Route::Setup.hash()>
                                                "Einrichten"
                                            </Button>
                                        </EmptyState>
                                    </li>
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <li class="ck-empty">
                                        <EmptyState
                                            title="Noch keine Tickets"
                                            text="Verbinde Jira und GitHub unter „Einrichten“, dann erscheinen Tickets und Pull Requests hier."
                                            icon=icons::TASKS
                                        >
                                            <Button
                                                size=ControlSize::Small
                                                variant=ButtonStyle::GlassProminent
                                                href=Route::Setup.hash()
                                            >
                                                "Einrichten"
                                            </Button>
                                        </EmptyState>
                                    </li>
                                }
                                    .into_any()
                            }
                        } else if list.is_empty() {
                            let hint = if matches!(filter, Filter::Snoozed) {
                                "Mit „s“ stellst du einen Eintrag zurück; er kommt zur gewählten Zeit wieder."
                            } else {
                                "Neue Einträge erscheinen hier, sobald Jira oder GitHub etwas melden."
                            };
                            view! {
                                <li class="ck-empty">
                                    <EmptyState title=empty text=hint icon=icons::CHECK />
                                </li>
                            }
                                .into_any()
                        } else {
                            let active = current.get().map(|i| i.id);
                            list.into_iter()
                                .map(|item| {
                                    let selected = active.as_deref() == Some(item.id.as_str());
                                    row(store, item, selected, detail_open)
                                })
                                .collect_view()
                                .into_any()
                        }
                    }}
                </ul>
                <p class="ck-keys t-footnote t-secondary">
                    <span><kbd>"↑"</kbd><kbd>"↓"</kbd>" wählen"</span>
                    <span><kbd>"e"</kbd>" erledigt"</span>
                    <span><kbd>"r"</kbd>" antworten"</span>
                    <span><kbd>"s"</kbd>" zurückstellen"</span>
                    <span><kbd>"u"</kbd>" zurückholen"</span>
                    <span><kbd>"/"</kbd>" suchen"</span>
                </p>
            </Pane>

            <Pane label="Details">
                <div class="ck-back">
                    <Button on_click=move |_| detail_open.set(false)>"‹ Zur Liste"</Button>
                </div>
                {move || match current.get() {
                    Some(item) => detail(store, item, items, draft, edit).into_any(),
                    None => {
                        view! {
                            <PaneBody>
                                <EmptyState
                                    title="Kein Eintrag gewählt"
                                    text="Wähle links ein Ticket oder einen Pull Request, dann siehst du hier Details und kannst antworten."
                                />
                            </PaneBody>
                        }
                            .into_any()
                    }
                }}
            </Pane>
        </SplitView>
    }
}

/// The entry with the reason the rules deliver: the most important hit determines icon,
/// color and sentence in the row. In "Zurückgestellt" the time until when is shown instead.
fn with_reason(w: &cockpit_core::Workspace, item: &Item, filter: Filter, now: u64) -> Item {
    let mut item = item.clone();
    if let Some(hit) = w.hits(&item, now).into_iter().next() {
        item.reason = hit.reason;
        item.reason_text = hit.text;
    }
    if filter == Filter::Snoozed
        && let Some(snooze) = w.snooze_of(&item.id)
    {
        item.reason_text = snooze_text(snooze.until_ms);
    }
    item
}

/// "Zurückgestellt bis Do., 12.11., 08:00" or "… bis sich etwas ändert".
fn snooze_text(until_ms: Option<u64>) -> String {
    match until_ms {
        Some(ms) => format!("Zurückgestellt bis {}", format_time(ms)),
        None => "Zurückgestellt, bis sich etwas ändert".to_string(),
    }
}

/// Time in the browser's local time, short and in German.
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
        .to_locale_string("de-DE", &options)
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

mod controls;
mod detail;
mod keyboard;
mod row;

use controls::{filter_bar, is_narrowed, project_picker, rule_switch, status_follow_switch};
use detail::detail;
use keyboard::{on_key, query_html};
use row::{row, row_icon};
