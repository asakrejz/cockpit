//! Aufmerksamkeit: links die Inbox aus Tickets und Pull Requests, rechts das Detail
//! mit Status, Branch, Kommentaren und dem Weg zum Agenten.

use cockpit_core::{Filter, Item, SourceFilter, TicketStatus};
use feature_ui::*;
use leptos::{ev, prelude::*, wasm_bindgen::JsCast};
use web_sys::{HtmlElement, KeyboardEvent, ScrollIntoViewOptions, ScrollLogicalPosition};

use crate::{
    route::Route,
    sidebar::AppSidebar,
    store::{Store, navigate, use_store},
    view,
};

#[component]
pub fn InboxPage(filter: Filter) -> impl IntoView {
    let store = use_store();

    // Die Memos melden sich nur, wenn sich ihr Ergebnis wirklich ändert. Ein tickender
    // Agent an einem anderen Ticket baut das Detail also nicht neu auf.
    // Suchtext und Quelle gehören nur dieser Seite; sie bleiben nicht über Navigation hinweg.
    let query = RwSignal::new(String::new());
    let source = RwSignal::new(SourceFilter::All);
    let items = Memo::new(move |_| {
        let (source, query) = (source.get(), query.get());
        store.ws.with(|w| {
            w.search(filter, source, &query)
                .into_iter()
                .cloned()
                .collect::<Vec<Item>>()
        })
    });
    // Der gewählte Eintrag. Ist er im Filter nicht sichtbar, gilt der erste.
    let current = Memo::new(move |_| {
        let id = store.selected.get();
        let list = items.get();
        list.iter().find(|i| i.id == id).or(list.first()).cloned()
    });

    // Entwurf der Antwort. Mit jedem Wechsel des Eintrags beginnt ein neuer.
    let draft = RwSignal::new(String::new());
    Effect::new(move |_| {
        store.selected.track();
        draft.set(String::new());
    });

    // Tastaturbedienung: gilt, solange die Seite offen ist, und endet mit ihr.
    let listener = window_event_listener(ev::keydown, move |event| {
        on_key(store, items, current, event)
    });
    on_cleanup(move || listener.remove());

    // Der gewählte Eintrag soll sichtbar bleiben, auch wenn er per Tastatur wechselt.
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
                            {move || store.ws.with(|w| w.unread(filter)).to_string()}
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
                        if list.is_empty() && is_narrowed(&query.get(), source.get()) {
                            let reset = move |_| {
                                query.set(String::new());
                                source.set(SourceFilter::All);
                            };
                            view! {
                                <li class="ck-empty ui-vstack">
                                    <p class="t-subheadline t-secondary">
                                        "Keine Treffer. Passe Suche oder Quelle an."
                                    </p>
                                    <Button size=ControlSize::Small on_click=reset>
                                        "Suche zurücksetzen"
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
                    <span><kbd>"/"</kbd>" suchen"</span>
                </p>
            </Pane>

            <Pane label="Details">
                {move || match current.get() {
                    Some(item) => detail(store, item, draft).into_any(),
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

/// Ist die Liste durch Suche oder Quelle eingeschränkt?
fn is_narrowed(query: &str, source: SourceFilter) -> bool {
    !query.trim().is_empty() || source != SourceFilter::All
}

/// Erstes Element, das zum Selektor passt.
fn query_html(selector: &str) -> Option<HtmlElement> {
    document()
        .query_selector(selector)
        .ok()
        .flatten()
        .and_then(|el| el.dyn_into().ok())
}

/// Setzt den Fokus auf ein Eingabefeld und markiert dessen Inhalt.
fn focus_field(selector: &str) {
    if let Some(el) = query_html(selector) {
        let _ = el.focus();
        if let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() {
            input.select();
        }
    }
}

/// Tippt der Nutzer gerade in ein Eingabefeld? Dann gehören die Tasten dem Feld.
fn is_typing(event: &KeyboardEvent) -> bool {
    event
        .target()
        .and_then(|t| t.dyn_into::<HtmlElement>().ok())
        .is_some_and(|el| {
            matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                || el.is_content_editable()
        })
}

/// Ein Tastendruck in der Inbox: ↑/↓ und j/k wechseln den Eintrag, `e` erledigt ihn,
/// `r` springt ins Antwortfeld, `/` in die Suche.
fn on_key(store: Store, items: Memo<Vec<Item>>, current: Memo<Option<Item>>, event: KeyboardEvent) {
    if event.ctrl_key() || event.meta_key() || event.alt_key() {
        return;
    }
    if is_typing(&event) {
        // Escape verlässt das Feld und gibt die Kürzel wieder frei.
        if event.key() == "Escape"
            && let Some(el) = event
                .target()
                .and_then(|t| t.dyn_into::<HtmlElement>().ok())
        {
            let _ = el.blur();
        }
        return;
    }
    let step: isize = match event.key().as_str() {
        "ArrowDown" | "j" => 1,
        "ArrowUp" | "k" => -1,
        "e" => {
            if let Some(item) = current.get_untracked() {
                store.ws.update(|w| w.mark_done(&item.id));
            }
            return;
        }
        "r" => {
            event.prevent_default();
            focus_field(".ck-reply textarea");
            return;
        }
        "/" => {
            event.prevent_default();
            focus_field(".ck-search input");
            return;
        }
        _ => return,
    };
    event.prevent_default();
    let list = items.get_untracked();
    let Some(now) = current.get_untracked() else {
        return;
    };
    let at = list.iter().position(|i| i.id == now.id).unwrap_or(0);
    if let Some(next) = at.checked_add_signed(step).and_then(|n| list.get(n)) {
        store.selected.set(next.id.clone());
    }
}

/// Eine Zeile der Inbox.
fn row(store: Store, item: Item, selected: bool) -> impl IntoView {
    let id = item.id.clone();
    view! {
        <InboxRow selected=selected on_select=move |_| store.selected.set(id.clone())>
            <UnreadDot read=item.read />
            <span class="ui-vstack" style="--spacing: 4px; flex: 1 1 auto; min-width: 0">
                <span class="ui-hstack" style="--spacing: 6px; flex-wrap: nowrap">
                    <Badge variant=BadgeVariant::Neutral>{item.source.label()}</Badge>
                    <span class="t-footnote t-mono t-secondary">{item.key}</span>
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

/// Icon am Anfang einer Zeile im Abschnitt „Entwicklung“.
fn row_icon(path: &'static str) -> impl IntoView {
    view! {
        <span style="display: inline-flex; flex: none; color: var(--sys-color-secondary-label)">
            <Icon path=path />
        </span>
    }
}

/// Das Detail zum gewählten Eintrag.
fn detail(store: Store, item: Item, draft: RwSignal<String>) -> impl IntoView {
    let id = StoredValue::new(item.id.clone());
    let is_ticket = item.is_ticket();
    let source = item.source.label();
    let review_href = item
        .pr
        .as_ref()
        .map(|pr| Route::Review(Some(pr.number)).hash());

    // Tickets zeigen ihren Status zum Umschalten, Pull Requests den Stand der Checks.
    let state = if is_ticket {
        view! {
            <SegmentedPicker
                options=TicketStatus::OPTIONS
                value=item.status.id()
                on_change=move |status: &'static str| {
                    if let Some(status) = TicketStatus::from_id(status) {
                        store.ws.update(|w| {
                            w.set_status(&id.get_value(), status);
                        });
                    }
                }
                aria_label="Status"
            />
        }
        .into_any()
    } else {
        item.pr
            .as_ref()
            .map(|pr| {
                let (label, tone) = pr.status();
                view! { <Status tone=view::tone(tone)>{label}</Status> }
            })
            .into_any()
    };

    let branch = match item.branch.clone() {
        Some(name) => view! { <Chip>{name}</Chip> }.into_any(),
        None => view! {
            <Button
                size=ControlSize::Small
                on_click=move |_| {
                    store.ws.update(|w| {
                        w.create_branch(&id.get_value());
                    })
                }
            >
                "Branch erstellen"
            </Button>
        }
        .into_any(),
    };

    // Die wichtigste Aktion: Agent beauftragen – oder, wenn es schon einen Pull Request gibt, prüfen.
    let primary = match (&review_href, is_ticket) {
        (Some(href), _) => view! {
            <Button variant=ButtonStyle::GlassProminent size=ControlSize::Large href=href.clone()>
                {if is_ticket { "Pull Request prüfen" } else { "Review öffnen" }}
            </Button>
        }
        .into_any(),
        (None, true) => view! {
            <Button
                variant=ButtonStyle::GlassProminent
                size=ControlSize::Large
                href=Route::Agent(Some(item.key.clone())).hash()
            >
                <Icon path=icons::SPARKLES />
                "Agent beauftragen"
            </Button>
        }
        .into_any(),
        (None, false) => ().into_any(),
    };

    let comments = item
        .comments
        .iter()
        .map(|c| {
            view! {
                <CommentBubble
                    who=c.author.name.clone()
                    initials=c.author.initials.clone()
                    time=c.time.clone()
                    text=c.text.clone()
                    own=c.own
                />
            }
        })
        .collect_view();

    let send = move |_| {
        let text = draft.get_untracked();
        if store.ws.try_update(|w| w.reply(&id.get_value(), &text)) == Some(true) {
            draft.set(String::new());
        }
    };

    view! {
        <PaneHeader>
            <div class="ui-hstack" style="--spacing: 10px">
                <Badge variant=BadgeVariant::Neutral>{source}</Badge>
                <span class="t-subheadline t-mono t-secondary">{item.key.clone()}</span>
                <span class="ui-spacer"></span>
                <span class="ui-hstack" style="--spacing: 8px; flex-wrap: nowrap">
                    <Avatar initials=item.owner.initials.clone() />
                    <span class="t-subheadline">{item.owner.name.clone()}</span>
                </span>
            </div>
            <h2 class="t-title1 t-bold">{item.title.clone()}</h2>
            {state}
        </PaneHeader>

        <PaneBody>
            <p class="t-body">{item.description.clone()}</p>

            <List>
                <ListSection header="Entwicklung" nested=true>
                    <li>
                        <div class="ui-list__row" style="flex-wrap: wrap">
                            {row_icon(icons::GIT_BRANCH)} <span class="ui-list__title">"Branch"</span> {branch}
                        </div>
                    </li>
                    <li>
                        <div class="ui-list__row" style="flex-wrap: wrap">
                            {row_icon(icons::GIT_PR)} <span class="ui-list__title">"Pull Request"</span>
                            <span class="ui-list__detail">
                                {item.pr.as_ref().map_or("Noch keiner".to_string(), |pr| pr.label())}
                            </span>
                            {(!is_ticket)
                                .then(|| review_href.clone())
                                .flatten()
                                .map(|href| {
                                    view! { <Button size=ControlSize::Small href=href>"Review öffnen"</Button> }
                                })}
                        </div>
                    </li>
                    {item
                        .agent_note
                        .clone()
                        .map(|note| {
                            view! {
                                <li>
                                    <div class="ui-list__row">
                                        <Avatar agent=true />
                                        <span class="ui-list__title">{note}</span>
                                    </div>
                                </li>
                            }
                        })}
                </ListSection>
            </List>

            <div class="ui-hstack" style="--spacing: 10px">
                {primary}
                <Button
                    size=ControlSize::Large
                    disabled=item.read
                    on_click=move |_| store.ws.update(|w| w.mark_done(&id.get_value()))
                >
                    {if item.read { "Erledigt" } else { "Als erledigt markieren" }}
                </Button>
            </div>

            <div class="ui-list__section">
                <p class="ui-list__header">"Kommentare"</p>
                <div class="ui-vstack" style="--spacing: 12px">
                    {comments}
                    <div class="ui-vstack" style="--spacing: 8px">
                        <div class="ck-reply">
                            <Textarea
                                value=draft
                                label="Antwort"
                                placeholder=if is_ticket { "Im Ticket antworten …" } else { "Im Pull Request antworten …" }
                            />
                        </div>
                        <div class="ui-hstack" style="justify-content: space-between">
                            <span class="t-footnote t-secondary">
                                {format!("Wird direkt in {source} gepostet.")}
                            </span>
                            <Button
                                variant=ButtonStyle::BorderedProminent
                                disabled=Signal::derive(move || draft.get().trim().is_empty())
                                on_click=send
                            >
                                "Antworten"
                            </Button>
                        </div>
                    </div>
                </div>
            </div>
        </PaneBody>
    }
}
