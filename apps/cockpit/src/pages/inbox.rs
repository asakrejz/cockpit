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
    route::Route,
    server::{self, CallError},
    sidebar::AppSidebar,
    store::{Store, navigate, now_ms, use_store, utc_offset_min},
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
    // Repo, person, status and label likewise; empty means "all".
    let repo = RwSignal::new(String::new());
    let person = RwSignal::new(String::new());
    let status = RwSignal::new(String::new());
    let label = RwSignal::new(String::new());
    let inbox_filter = Memo::new(move |_| InboxFilter {
        repo: some_if_set(repo.get()),
        person: some_if_set(person.get()),
        status: TicketStatus::from_id(&status.get()),
        label: some_if_set(label.get()),
    });
    let items = Memo::new(move |_| {
        let (source, query, now) = (source.get(), query.get(), store.now.get());
        let inbox_filter = inbox_filter.get();
        store.ws.with(|w| {
            inbox_filter
                .apply(w.search(filter, source, &query, now))
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

    let (title, empty) = match filter {
        Filter::All => ("Aufmerksamkeit", "Nichts wartet auf dich."),
        Filter::Mine => ("Meine Aufgaben", "Du hast gerade keine Aufgaben."),
        Filter::Team => ("Team", "Im Team ist gerade nichts offen."),
        Filter::Snoozed => ("Zurückgestellt", "Nichts ist zurückgestellt."),
    };
    let connection = move |label: &'static str,
                           on: fn(&cockpit_core::Connections) -> bool,
                           text: &'static str| {
        let connected = Signal::derive(move || on(&store.conn.get()));
        let status = view! {
            <Status tone=Signal::derive(move || {
                if connected.get() { StatusTone::Green } else { StatusTone::Gray }
            })>{move || if connected.get() { text } else { "Getrennt" }}</Status>
        };
        view! { <SidebarItem href=Route::Setup.hash() label=label trailing=status.into_any() /> }
    };

    view! {
        <SplitView>
            <AppSidebar>
                <SidebarSection label="Regeln">
                    {Rule::ALL.into_iter().map(|rule| rule_switch(store, rule)).collect_view()}
                </SidebarSection>
                <SidebarSection label="Tickets">
                    {status_follow_switch(store)}
                </SidebarSection>
                <SidebarSection label="Verbindungen">
                    {connection("Jira", |c| c.jira, "Verbunden")}
                    {connection("GitHub", |c| c.github, "Verbunden")}
                    {connection("Agent", |c| c.agent, "Bereit")}
                </SidebarSection>
            </AppSidebar>

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
                        {filter_select(
                            "Repo",
                            "Alle Repos",
                            repo,
                            Signal::derive(move || {
                                store.ws.with(|w| w.item_repos()).into_iter().map(|r| (r.clone(), r)).collect()
                            }),
                        )}
                        {filter_select(
                            "Person",
                            "Alle Personen",
                            person,
                            Signal::derive(move || {
                                store
                                    .ws
                                    .with(|w| w.owners())
                                    .into_iter()
                                    .map(|p| (p.name.clone(), p.name))
                                    .collect()
                            }),
                        )}
                        {filter_select(
                            "Status",
                            "Alle Status",
                            status,
                            Signal::derive(|| {
                                TicketStatus::OPTIONS
                                    .iter()
                                    .map(|(id, l)| (id.to_string(), l.to_string()))
                                    .collect()
                            }),
                        )}
                        {filter_select(
                            "Label",
                            "Alle Labels",
                            label,
                            Signal::derive(move || {
                                store.ws.with(|w| w.labels()).into_iter().map(|l| (l.clone(), l)).collect()
                            }),
                        )}
                        <Show when=move || !inbox_filter.get().is_default()>
                            <Button
                                size=ControlSize::Small
                                on_click=move |_| {
                                    repo.set(String::new());
                                    person.set(String::new());
                                    status.set(String::new());
                                    label.set(String::new());
                                }
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
                                repo.set(String::new());
                                person.set(String::new());
                                status.set(String::new());
                                label.set(String::new());
                            };
                            view! {
                                <li class="ck-empty ui-vstack">
                                    <p class="t-subheadline t-secondary">
                                        "Keine Treffer. Passe Suche, Quelle oder Filter an."
                                    </p>
                                    <Button size=ControlSize::Small on_click=reset>
                                        "Alles zurücksetzen"
                                    </Button>
                                </li>
                            }
                                .into_any()
                        } else if list.is_empty() && store.ws.with(|w| w.items.is_empty()) {
                            // A fresh workspace: nothing connected or synced yet.
                            view! {
                                <li class="ck-empty ui-vstack">
                                    <p class="t-subheadline t-secondary">
                                        "Noch keine Tickets oder Pull Requests. Verbinde Jira und GitHub unter „Einrichten“, dann erscheinen sie hier."
                                    </p>
                                    <Button size=ControlSize::Small href=Route::Setup.hash()>
                                        "Einrichten"
                                    </Button>
                                </li>
                            }
                                .into_any()
                        } else if list.is_empty() {
                            view! { <li class="ck-empty t-subheadline t-secondary">{empty}</li> }.into_any()
                        } else {
                            let active = current.get().map(|i| i.id);
                            list.into_iter()
                                .map(|item| {
                                    let selected = active.as_deref() == Some(item.id.as_str());
                                    row(store, item, selected)
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
                {move || match current.get() {
                    Some(item) => detail(store, item, items, draft, edit).into_any(),
                    None => {
                        view! {
                            <PaneHeader>
                                <p class="t-subheadline t-secondary">"Kein Eintrag gewählt."</p>
                            </PaneHeader>
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

use controls::{
    filter_select, is_narrowed, project_picker, rule_switch, some_if_set, status_follow_switch,
};
use detail::detail;
use keyboard::{on_key, query_html};
use row::{row, row_icon};
