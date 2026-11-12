//! Aufmerksamkeit: links die Inbox aus Tickets und Pull Requests, rechts das Detail
//! mit Status, Branch, Kommentaren und dem Weg zum Agenten.

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

    // Die Memos melden sich nur, wenn sich ihr Ergebnis wirklich ändert. Ein tickender
    // Agent an einem anderen Ticket baut das Detail also nicht neu auf.
    // Suchtext und Quelle gehören nur dieser Seite; sie bleiben nicht über Navigation hinweg.
    let query = RwSignal::new(String::new());
    let source = RwSignal::new(SourceFilter::All);
    // Repo, Person, Status und Label ebenso; leer heißt „alle“.
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

    // Bearbeiten-Modus eines Tickets: Entwurf von Titel und Beschreibung. Mit jedem Wechsel
    // des Eintrags endet er; ein Entwurf geht nicht in ein anderes Ticket über.
    let edit = Edit {
        on: RwSignal::new(false),
        title: RwSignal::new(String::new()),
        description: RwSignal::new(String::new()),
    };
    Effect::new(move |_| {
        store.selected.track();
        edit.on.set(false);
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
                            // Ein frischer Workspace: noch nichts verbunden oder abgeglichen.
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

/// Auswahl des Jira-Projekts. Die Liste kommt von Jira (mit Namen), dazu die
/// Projekte der Tickets, die schon da sind. Ohne Projekte bleibt sie
/// weg. Die Wahl ist eine Action: Der Server gleicht danach gleich ab.
fn project_picker(store: Store) -> impl IntoView {
    let remote = RwSignal::new(Vec::<cockpit_core::api::JiraProject>::new());
    leptos::task::spawn_local(async move {
        match server::fetch_jira_projects().await {
            Ok(r) => remote.set(r.projects),
            // Ist der Server nicht erreichbar, sagt das schon der Hinweis oben.
            Err(CallError::Unreachable) => {}
            Err(e) => store.notify(format!("Jira-Projekte nicht geladen: {}", e.message())),
        }
    });
    let options = Signal::derive(move || {
        let mut list: Vec<(String, String)> = remote.with(|r| {
            r.iter()
                .map(|p| {
                    let label = if p.name.is_empty() {
                        p.key.clone()
                    } else {
                        format!("{} · {}", p.key, p.name)
                    };
                    (p.key.clone(), label)
                })
                .collect()
        });
        for key in store.ws.with(|w| w.projects()) {
            if !list.iter().any(|(k, _)| *k == key) {
                list.push((key.clone(), key));
            }
        }
        list.sort_by_key(|a| a.1.to_lowercase());
        list
    });
    let value = Signal::derive(move || {
        store
            .ws
            .with(|w| w.jira_project.clone().unwrap_or_default())
    });
    let on_change = move |key: String| {
        let project = (!key.is_empty()).then_some(key);
        if let Err(e) = store.dispatch(Action::SelectJiraProject { project }) {
            store.notify(e.message());
        }
    };
    move || {
        let list = options.get();
        (!list.is_empty()).then(|| {
            let all = std::iter::once((String::new(), "Alle Projekte".to_string()))
                .chain(list)
                .collect::<Vec<_>>();
            view! {
                <div class="ck-project">
                    <Select
                        options=all
                        value=value
                        on_change=on_change
                        aria_label="Jira-Projekt"
                        compact=true
                    />
                </div>
            }
        })
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

/// Der Schalter „Status folgt dem Pull Request“. Wie bei den Regeln liegt der Zustand im
/// Workspace und ändert sich nur per Action.
fn status_follow_switch(store: Store) -> impl IntoView {
    let on = RwSignal::new(store.ws.with_untracked(|w| !w.pr_status_off));
    Effect::new(move |_| {
        let enabled = store.ws.with(|w| !w.pr_status_off);
        if on.get_untracked() != enabled {
            on.set(enabled);
        }
    });
    Effect::new(move |previous: Option<()>| {
        let enabled = on.get();
        if previous.is_some() {
            let _ = store.dispatch(Action::SetStatusFollowsPr { enabled });
        }
    });
    view! {
        <li
            class="ck-rule"
            title="Pull Request als Entwurf oder offen: In Arbeit. Review angefragt: Review. Gemergt: Fertig."
        >
            <label class="t-subheadline">
                <span>"Status folgt dem Pull Request"</span>
                <Toggle checked=on aria_label="Status folgt dem Pull Request" />
            </label>
        </li>
    }
}

/// Entwurf im Bearbeiten-Modus eines Tickets.
#[derive(Clone, Copy)]
struct Edit {
    on: RwSignal<bool>,
    title: RwSignal<String>,
    description: RwSignal<String>,
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
fn is_narrowed(query: &str, source: SourceFilter, filter: &InboxFilter) -> bool {
    !query.trim().is_empty() || source != SourceFilter::All || !filter.is_default()
}

fn some_if_set(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

/// Eine Auswahl der Filterleiste. Der leere Wert heißt „alle“, `all` ist seine Beschriftung.
/// Ein gewählter Wert, der nicht mehr vorkommt (etwa nach einem Abgleich), bleibt in der
/// Liste, damit die Anzeige zum Filter passt.
fn filter_select(
    aria_label: &'static str,
    all: &'static str,
    value: RwSignal<String>,
    choices: Signal<Vec<(String, String)>>,
) -> impl IntoView {
    let options = Signal::derive(move || {
        let mut list = choices.get();
        let current = value.get();
        if !current.is_empty() && !list.iter().any(|(v, _)| *v == current) {
            list.push((current.clone(), current));
        }
        std::iter::once((String::new(), all.to_string()))
            .chain(list)
            .collect::<Vec<_>>()
    });
    view! {
        <Select
            options=options
            value=value
            on_change=move |v: String| value.set(v)
            aria_label=aria_label
            compact=true
        />
    }
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
    edit: Edit,
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

    // Ticket bearbeiten: Titel und Beschreibung, Zuweisung, Priorität.
    let initial = StoredValue::new((item.title.clone(), item.description.clone()));
    let start = move |_| {
        let (title, description) = initial.get_value();
        edit.title.set(title);
        edit.description.set(description);
        edit.on.set(true);
    };
    let save = move |_| {
        let saved = store.dispatch(Action::EditTicket {
            item: id.get_value(),
            title: edit.title.get_untracked(),
            description: edit.description.get_untracked(),
        });
        // „Es ändert sich nichts“ ist kein Grund, im Bearbeiten-Modus zu bleiben.
        if saved.is_ok() || matches!(saved, Err(cockpit_core::ActionError::Unchanged)) {
            edit.on.set(false);
        }
    };
    let cancel = move |_| edit.on.set(false);
    let edit_button = is_ticket.then(|| {
        view! {
            <Button size=ControlSize::Small on_click=start>
                "Bearbeiten"
            </Button>
        }
    });
    let edit_form = is_ticket.then(|| {
        view! {
            <Show when=move || edit.on.get()>
                <div class="ck-edit ui-vstack" style="--spacing: 12px">
                    <TextField
                        label="Titel"
                        value=edit.title
                        hint="Der Titel darf nicht leer sein."
                        error=Signal::derive(move || edit.title.get().trim().is_empty())
                    />
                    <Textarea value=edit.description label="Beschreibung" />
                    <div class="ui-hstack" style="--spacing: 8px; justify-content: flex-end">
                        <Button on_click=cancel>"Abbrechen"</Button>
                        <Button
                            variant=ButtonStyle::BorderedProminent
                            disabled=Signal::derive(move || edit.title.get().trim().is_empty())
                            on_click=save
                        >
                            "Speichern"
                        </Button>
                    </div>
                </div>
            </Show>
        }
    });

    let ticket_props = is_ticket.then(|| {
        let people = store.ws.with_untracked(|w| w.people());
        let people = people
            .into_iter()
            .map(|person: Person| {
                let current = person == item.owner;
                let label = person.name.clone();
                view! {
                    <Button
                        size=ControlSize::Small
                        variant=if current { ButtonStyle::BorderedProminent } else { ButtonStyle::Bordered }
                        on_click=move |_| {
                            let _ = store.dispatch(Action::AssignTicket {
                                item: id.get_value(),
                                person: person.clone(),
                            });
                        }
                    >
                        {label}
                    </Button>
                }
            })
            .collect_view();
        view! {
            <ListSection header="Ticket" nested=true>
                <li>
                    <div class="ui-list__row" style="flex-wrap: wrap">
                        <span class="ui-list__title">"Priorität"</span>
                        <SegmentedPicker
                            options=Priority::OPTIONS
                            value=item.priority.id()
                            on_change=move |p: &'static str| {
                                if let Some(priority) = Priority::from_id(p) {
                                    let _ = store.dispatch(Action::SetPriority {
                                        item: id.get_value(),
                                        priority,
                                    });
                                }
                            }
                            aria_label="Priorität"
                        />
                    </div>
                </li>
                <li>
                    <div class="ui-list__row" style="flex-wrap: wrap">
                        <span class="ui-list__title">"Zugewiesen an"</span>
                        <span class="ck-assignees ui-hstack" style="--spacing: 6px" role="group" aria-label="Zugewiesen an">
                            {people}
                        </span>
                    </div>
                </li>
            </ListSection>
        }
    });

    // Verknüpfung: am Ticket der Pull Request (lösen oder von Hand verknüpfen), am Pull Request das Ticket.
    let link_actions = if is_ticket {
        match item.pr.as_ref() {
            Some(_) => view! {
                <Button
                    size=ControlSize::Small
                    on_click=move |_| {
                        let _ = store.dispatch(Action::UnlinkPr { item: id.get_value() });
                    }
                >
                    "Verknüpfung lösen"
                </Button>
            }
            .into_any(),
            None => {
                let open = store.ws.with_untracked(|w| {
                    w.reviews
                        .iter()
                        .filter(|r| r.merged.is_none() && w.ticket_of_pr(r.number).is_none())
                        .map(|r| (r.number, r.title.clone()))
                        .collect::<Vec<_>>()
                });
                open.into_iter()
                    .map(|(number, title)| {
                        view! {
                            <Button
                                size=ControlSize::Small
                                on_click=move |_| {
                                    let _ = store.dispatch(Action::LinkPr {
                                        item: id.get_value(),
                                        pr: number,
                                    });
                                }
                            >
                                {format!("Mit #{number} verknüpfen")}
                            </Button>
                        }
                        .attr("title", title)
                    })
                    .collect_view()
                    .into_any()
            }
        }
    } else {
        ().into_any()
    };
    let linked_ticket = (!is_ticket)
        .then(|| {
            let number = item.pr.as_ref()?.number;
            store.ws.with_untracked(|w| {
                w.ticket_of_pr(number)
                    .map(|t| (t.id.clone(), t.key.clone(), t.title.clone()))
            })
        })
        .flatten()
        .map(|(ticket_id, key, title)| {
            view! {
                <li>
                    <div class="ui-list__row" style="flex-wrap: wrap">
                        {row_icon(icons::GIT_PR)} <span class="ui-list__title">"Ticket"</span>
                        <span class="ui-list__detail ck-ellipsis">{format!("{key} · {title}")}</span>
                        <Button
                            size=ControlSize::Small
                            on_click=move |_| store.open_item(&ticket_id)
                        >
                            {format!("{key} öffnen")}
                        </Button>
                    </div>
                </li>
            }
        });

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
            <div class="ui-hstack" style="justify-content: space-between; align-items: flex-start">
                <h2 class="t-title1 t-bold">{item.title.clone()}</h2>
                {edit_button}
            </div>
            {state}
        </PaneHeader>

        <PaneBody>
            {edit_form}
            <p class="t-body" hidden=move || edit.on.get() && is_ticket>{item.description.clone()}</p>

            <List>{ticket_props}</List>

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
                            {link_actions}
                            {(!is_ticket)
                                .then(|| review_href.clone())
                                .flatten()
                                .map(|href| {
                                    view! { <Button size=ControlSize::Small href=href>"Review öffnen"</Button> }
                                })}
                        </div>
                    </li>
                    {linked_ticket}
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
