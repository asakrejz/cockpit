//! Aufmerksamkeit: links die Inbox aus Tickets und Pull Requests, rechts das Detail
//! mit Status, Branch, Kommentaren und dem Weg zum Agenten.

use cockpit_core::{Action, Filter, Item, Rule, SnoozeChoice, SourceFilter, TicketStatus};
use feature_ui::*;
use leptos::{ev, prelude::*, wasm_bindgen::JsCast};
use web_sys::{HtmlElement, KeyboardEvent, ScrollIntoViewOptions, ScrollLogicalPosition};

use crate::{
    route::Route,
    sidebar::AppSidebar,
    store::{Store, navigate, now_ms, use_store, utc_offset_min},
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
        let (source, query, now) = (source.get(), query.get(), store.now.get());
        store.ws.with(|w| {
            w.search(filter, source, &query, now)
                .into_iter()
                .map(|item| with_reason(w, item, filter, now))
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
                    <span><kbd>"s"</kbd>" zurückstellen"</span>
                    <span><kbd>"u"</kbd>" zurückholen"</span>
                    <span><kbd>"/"</kbd>" suchen"</span>
                </p>
            </Pane>

            <Pane label="Details">
                {move || match current.get() {
                    Some(item) => detail(store, item, items, draft).into_any(),
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

/// Der Eintrag mit dem Grund, den die Regeln liefern: Der wichtigste Treffer bestimmt Icon,
/// Farbe und Satz in der Zeile. In „Zurückgestellt“ steht stattdessen, bis wann.
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

/// „Zurückgestellt bis Do., 12.11., 08:00“ oder „… bis sich etwas ändert“.
fn snooze_text(until_ms: Option<u64>) -> String {
    match until_ms {
        Some(ms) => format!("Zurückgestellt bis {}", format_time(ms)),
        None => "Zurückgestellt, bis sich etwas ändert".to_string(),
    }
}

/// Zeitpunkt in der Ortszeit des Browsers, kurz und deutsch.
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

/// Ein Schalter für eine Regel. Der Zustand liegt im Workspace und ändert sich nur per Action;
/// der Schalter spiegelt ihn.
fn rule_switch(store: Store, rule: Rule) -> impl IntoView {
    let on = RwSignal::new(store.ws.with_untracked(|w| w.rules.enabled(rule)));
    // Workspace → Schalter (z. B. nach dem Zurücksetzen der Demo).
    Effect::new(move |_| {
        let enabled = store.ws.with(|w| w.rules.enabled(rule));
        if on.get_untracked() != enabled {
            on.set(enabled);
        }
    });
    // Schalter → Workspace. Der erste Lauf ändert nichts.
    Effect::new(move |previous: Option<()>| {
        let enabled = on.get();
        if previous.is_some() {
            let _ = store.dispatch(Action::SetRule { rule, enabled });
        }
    });
    view! {
        <li class="ck-rule" title=rule.describe()>
            <label class="t-subheadline">
                <span>{rule.label()}</span>
                <Toggle checked=on aria_label=rule.label() />
            </label>
        </li>
    }
}

/// Stellt einen Eintrag zurück und wählt danach den Nachbarn in der Liste.
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
/// `r` springt ins Antwortfeld, `s` zu den Möglichkeiten zum Zurückstellen, `u` holt einen
/// zurückgestellten Eintrag zurück, `/` springt in die Suche.
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
                let _ = store.dispatch(Action::MarkDone {
                    item: item.id.clone(),
                });
            }
            return;
        }
        "r" => {
            event.prevent_default();
            focus_field(".ck-reply textarea");
            return;
        }
        "s" => {
            event.prevent_default();
            if let Some(button) = query_html(".ck-snooze button") {
                let _ = button.focus();
            }
            return;
        }
        "u" => {
            if let Some(item) = current.get_untracked() {
                let _ = store.dispatch(Action::Unsnooze { item: item.id });
            }
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
fn detail(
    store: Store,
    item: Item,
    items: Memo<Vec<Item>>,
    draft: RwSignal<String>,
) -> impl IntoView {
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
                        let _ = store.dispatch(Action::SetStatus {
                            item: id.get_value(),
                            status,
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
                    let _ = store.dispatch(Action::CreateBranch { item: id.get_value() });
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

    // Zurückstellen: vier Möglichkeiten; ist der Eintrag schon zurückgestellt, steht dort wie lange.
    let snoozed = store.ws.with_untracked(|w| w.snooze_of(&item.id).cloned());
    let snooze_row = match snoozed {
        Some(snooze) => view! {
            <div class="ck-snooze ui-hstack" style="--spacing: 10px; justify-content: space-between">
                <span class="t-subheadline t-secondary">{snooze_text(snooze.until_ms)}</span>
                <Button
                    size=ControlSize::Small
                    on_click=move |_| {
                        let _ = store.dispatch(Action::Unsnooze { item: id.get_value() });
                    }
                >
                    "Zurückholen"
                </Button>
            </div>
        }
        .into_any(),
        None => view! {
            <div class="ck-snooze ui-hstack" style="--spacing: 8px" role="group" aria-label="Zurückstellen">
                <span class="t-subheadline t-secondary">"Zurückstellen:"</span>
                {SnoozeChoice::ALL
                    .into_iter()
                    .map(|choice| {
                        let label = SnoozeChoice::OPTIONS
                            .iter()
                            .find(|(key, _)| *key == choice.id())
                            .map_or("", |(_, label)| label);
                        view! {
                            <Button
                                size=ControlSize::Small
                                on_click=move |_| snooze(store, items, &id.get_value(), choice)
                            >
                                {label}
                            </Button>
                        }
                    })
                    .collect_view()}
            </div>
        }
        .into_any(),
    };

    let send = move |_| {
        let text = draft.get_untracked();
        let sent = store.dispatch(Action::Reply {
            item: id.get_value(),
            text,
        });
        if sent.is_ok() {
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
                    on_click=move |_| {
                        let _ = store.dispatch(Action::MarkDone { item: id.get_value() });
                    }
                >
                    {if item.read { "Erledigt" } else { "Als erledigt markieren" }}
                </Button>
            </div>

            {snooze_row}

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
