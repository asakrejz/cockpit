//! The filter button, the chips of the active filters and the filter dialog. The dialog renders
//! the central list `cockpit_core::FILTERS`; the values are applied together by
//! `Action::SetInboxFilter` (display only – nothing is written to Jira or GitHub).

use cockpit_core::{
    ASSIGNED_TO_ME, Applies, Choices, FILTERS, FilterDef, InboxFilter, JIRA_PROJECT_FILTER,
    NO_TICKET, UNASSIGNED,
};

use super::controls::{project_options, rule_switch, status_follow_switch};
use super::*;

/// Reports an error of an action, except „nothing changed“.
fn report(store: Store, result: Result<cockpit_core::Applied, cockpit_core::ActionError>) {
    if let Err(e) = result
        && e != cockpit_core::ActionError::Unchanged
    {
        store.notify(e.msg().render(feature_ui::lang_untracked()));
    }
}

/// The name of a filter of the central list, in the current language.
fn filter_label(id: &str) -> String {
    match id {
        JIRA_PROJECT_FILTER => t!(InboxFiltersJiraProject),
        "source" => t!(InboxFiltersLabelSource),
        "sprint" => t!(InboxFiltersLabelSprint),
        "project" => t!(InboxFiltersLabelProject),
        "repo" => t!(InboxFiltersLabelRepo),
        "person" => t!(InboxFiltersLabelPerson),
        "status" => t!(InboxFiltersLabelStatus),
        "label" => t!(InboxFiltersLabelLabel),
        "reporter" => t!(InboxFiltersLabelReporter),
        "watched" => t!(InboxFiltersLabelWatched),
        "type" => t!(InboxFiltersLabelType),
        "priority" => t!(InboxFiltersLabelPriority),
        "done" => t!(InboxFiltersLabelDone),
        other => other.to_string(),
    }
}

/// The „no restriction“ choice of a filter.
fn all_label(id: &str) -> String {
    match id {
        "project" => t!(InboxFiltersAllProject),
        "repo" => t!(InboxFiltersAllRepo),
        "person" => t!(InboxFiltersAllPerson),
        "status" => t!(InboxFiltersAllStatus),
        "label" => t!(InboxFiltersAllLabel),
        "reporter" => t!(InboxFiltersAllReporter),
        "type" => t!(InboxFiltersAllType),
        "priority" => t!(InboxFiltersAllPriority),
        _ => t!(InboxFiltersOptAll),
    }
}

/// The text of one choice. Choices from the data (repos, labels, names, source names) keep
/// the text `cockpit_core` gives them.
fn choice_label(id: &str, value: &str, fallback: &str) -> String {
    match (id, value) {
        ("source", "all") => t!(InboxFiltersOptAll),
        ("source", "jira") => cockpit_core::Source::Jira.label().to_string(),
        ("source", "github") => cockpit_core::Source::GitHub.label().to_string(),
        ("source", "project") => cockpit_core::Source::GitHubProject.label().to_string(),
        ("sprint", "current") => t!(InboxFiltersOptSprintCurrent),
        ("sprint", "open") => t!(InboxFiltersOptSprintOpen),
        ("sprint", "recent") => t!(InboxFiltersOptSprintRecent),
        ("sprint", "backlog") => t!(InboxFiltersOptSprintBacklog),
        ("status", status) => TicketStatus::from_id(status)
            .map(labels::ticket_status)
            .unwrap_or_else(|| fallback.to_string()),
        ("done", "on") => t!(InboxFiltersOptDoneOn),
        ("done", "off") => t!(InboxFiltersOptDoneOff),
        ("watched", "off") => t!(InboxFiltersOptWatchedOff),
        ("watched", "on") => t!(InboxFiltersOptWatchedOn),
        ("priority", id) => cockpit_core::Priority::from_id(id)
            .map(labels::priority)
            .unwrap_or_else(|| fallback.to_string()),
        ("person" | "reporter", ASSIGNED_TO_ME) => t!(InboxFiltersPersonMe),
        ("person", UNASSIGNED) => t!(InboxFiltersPersonUnassigned),
        ("project", NO_TICKET) => t!(InboxFiltersProjectNoTicket),
        _ => fallback.to_string(),
    }
}

/// The button „Filter“ with the number of active filters.
pub(super) fn filter_button(store: Store, open: RwSignal<bool>) -> impl IntoView {
    let count = Signal::derive(move || store.ws.with(|w| w.active_filters().len()));
    view! {
        <Button on_click=move |_| open.set(true) aria_label=t!(InboxFiltersOpen)>
            {t!(InboxFiltersButton)}
            <Show when=move || { count.get() > 0 }>
                <Badge variant=BadgeVariant::Count>{move || count.get().to_string()}</Badge>
            </Show>
        </Button>
    }
}

/// One removable chip per active filter.
pub(super) fn filter_chips(store: Store) -> impl IntoView {
    move || {
        let list = store.ws.with(|w| w.active_filters());
        (!list.is_empty()).then(|| {
            let chips = list
                .into_iter()
                .map(|active| {
                    let id = active.id;
                    let remove = move |_| {
                        if id == JIRA_PROJECT_FILTER {
                            report(store, store.dispatch(Action::SelectJiraProject { project: None }));
                        } else {
                            let mut filter = store.ws.with_untracked(|w| w.inbox_filter.clone());
                            filter.reset(id);
                            report(store, store.dispatch(Action::SetInboxFilter { filter }));
                        }
                    };
                    let value = if id == JIRA_PROJECT_FILTER {
                        active.value.clone()
                    } else {
                        let raw = store.ws.with(|w| w.inbox_filter.get(id));
                        choice_label(id, &raw, &active.value)
                    };
                    let name = t!(
                        InboxFiltersChipPair,
                        label = filter_label(id),
                        value = value
                    );
                    let label = t!(InboxFiltersChipRemove, name = name);
                    view! {
                        <li class="ui-chip ck-chip">
                            <span>{name}</span>
                            <button type="button" class="ck-chip__remove" aria-label=label on:click=remove>
                                <Icon path=icons::CLOSE size=12 stroke=2.6 />
                            </button>
                        </li>
                    }
                })
                .collect_view();
            view! { <ul class="ck-chips" aria-label=t!(InboxFiltersActive)>{chips}</ul> }
        })
    }
}

/// The dialog with all filters. Edits go to a draft; "Apply" applies it, "Reset"
/// sets the draft back to the start values. The attention rules are switches that act at once.
pub(super) fn filter_dialog(store: Store, open: RwSignal<bool>) -> impl IntoView {
    let draft = RwSignal::new(InboxFilter::default());
    let jira = RwSignal::new(String::new());
    // Every time the dialog opens it starts from what is applied.
    Effect::new(move |_| {
        if open.get() {
            draft.set(store.ws.with_untracked(|w| w.inbox_filter.clone()));
            jira.set(
                store
                    .ws
                    .with_untracked(|w| w.jira_project.clone().unwrap_or_default()),
            );
        }
    });
    let projects = project_options(store);
    let apply = move |_| {
        let filter = draft.get_untracked();
        report(store, store.dispatch(Action::SetInboxFilter { filter }));
        let chosen = jira.get_untracked();
        let project = (!chosen.is_empty()).then_some(chosen);
        if project != store.ws.with_untracked(|w| w.jira_project.clone()) {
            report(store, store.dispatch(Action::SelectJiraProject { project }));
        }
        open.set(false);
    };
    let reset = move |_| {
        draft.set(InboxFilter::default());
        jira.set(String::new());
    };
    let body = move || {
        view! {
            <div class="ck-dialog">
                <div class="ck-dialog__field">
                    <Select
                        label=t!(InboxFiltersJiraProjectField)
                        options=projects
                        value=jira
                        on_change=move |v: String| jira.set(v)
                    />
                </div>
                {FILTERS.iter().map(|def| filter_field(store, draft, def)).collect_view()}
                <h2 class="t-headline ck-dialog__heading">{t!(InboxFiltersWhenNoticed)}</h2>
                <p class="t-footnote t-secondary">{t!(InboxFiltersSwitchesInstant)}</p>
                <ul class="ui-sidebar__nav">
                    {Rule::ALL.into_iter().map(|rule| rule_switch(store, rule)).collect_view()}
                    {status_follow_switch(store)}
                </ul>
            </div>
        }
    };
    let footer = move || {
        view! {
            <Button on_click=reset>{t!(InboxFiltersReset)}</Button>
            <Button variant=ButtonStyle::GlassProminent on_click=apply>
                {t!(InboxFiltersApply)}
            </Button>
        }
    };
    view! {
        <Dialog open=open title=t!(InboxFiltersDialogTitle) footer=footer>
            {body}
        </Dialog>
    }
}

/// The options of a filter: the fixed list or the choices from the data, plus the „all“
/// choice. A chosen value that no longer occurs in the data stays in the list.
fn options_of(
    store: Store,
    draft: RwSignal<InboxFilter>,
    def: &'static FilterDef,
) -> Signal<Vec<(String, String)>> {
    Signal::derive(move || {
        let mut list: Vec<(String, String)> = match def.choices {
            Choices::Fixed(fixed) => fixed
                .iter()
                .map(|v| (v.to_string(), choice_label(def.id, v, v)))
                .collect(),
            Choices::FromData => store
                .ws
                .with(|w| w.filter_choices(def.id))
                .into_iter()
                .map(|(v, l)| {
                    let label = choice_label(def.id, &v, &l);
                    (v, label)
                })
                .collect(),
        };
        let current = draft.with(|f| f.get(def.id));
        if !current.is_empty() && !list.iter().any(|(v, _)| *v == current) {
            list.push((current.clone(), current));
        }
        if def.has_all {
            std::iter::once((String::new(), all_label(def.id)))
                .chain(list)
                .collect()
        } else {
            list
        }
    })
}

/// The control for one filter of the central list.
fn filter_field(
    store: Store,
    draft: RwSignal<InboxFilter>,
    def: &'static FilterDef,
) -> impl IntoView {
    let value = Signal::derive(move || draft.with(|f| f.get(def.id)));
    let options = options_of(store, draft, def);
    let set = move |v: String| {
        draft.update(|f| {
            f.set(def.id, &v);
        })
    };
    // Filters of tickets only make no sense while there are no tickets.
    let shown = move || {
        def.applies != Applies::Tickets || store.ws.with(|w| w.items.iter().any(Item::is_ticket))
    };
    view! {
        <Show when=shown>
            <div class="ck-dialog__field">
                {if matches!(def.id, "person" | "reporter") {
                    view! { <SearchSelect label=filter_label(def.id) options=options value=value on_change=set /> }
                        .into_any()
                } else {
                    view! { <Select label=filter_label(def.id) options=options value=value on_change=set /> }
                        .into_any()
                }}
            </div>
        </Show>
    }
}

/// A dropdown with a search field: the button shows the choice, the list below it narrows
/// while typing.
#[component]
fn SearchSelect(
    label: String,
    #[prop(into)] options: Signal<Vec<(String, String)>>,
    #[prop(into)] value: Signal<String>,
    #[prop(into)] on_change: Callback<String>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    let shown = move || {
        let current = value.get();
        options
            .get()
            .into_iter()
            .find(|(v, _)| *v == current)
            .map_or(current, |(_, l)| l)
    };
    let matching = move || {
        let needle = query.get().trim().to_lowercase();
        options
            .get()
            .into_iter()
            .filter(|(_, l)| needle.is_empty() || l.to_lowercase().contains(&needle))
            .collect::<Vec<_>>()
    };
    view! {
        <div class="ck-combo">
            <span class="ui-field__label">{label.clone()}</span>
            <button
                type="button"
                class="ui-field__input ui-select ck-combo__button"
                aria-haspopup="listbox"
                aria-expanded=move || open.get().to_string()
                on:click=move |_| open.update(|o| *o = !*o)
            >
                {shown}
            </button>
            <Show when=move || open.get()>
                <div class="ck-combo__panel">
                    <SearchField value=query placeholder=t!(InboxFiltersSearchPerson) />
                    <ul class="ck-combo__list" role="listbox" aria-label=label.clone()>
                        {move || {
                            let list = matching();
                            if list.is_empty() {
                                view! { <li class="t-footnote t-secondary ck-combo__empty">{t!(InboxFiltersNoResults)}</li> }
                                    .into_any()
                            } else {
                                let current = value.get();
                                list.into_iter()
                                    .map(|(v, l)| {
                                        let selected = v == current;
                                        view! {
                                            <li role="option" aria-selected=selected.to_string()>
                                                <button
                                                    type="button"
                                                    class="ck-combo__option"
                                                    data-selected=selected.then_some("")
                                                    on:click=move |_| {
                                                        on_change.run(v.clone());
                                                        query.set(String::new());
                                                        open.set(false);
                                                    }
                                                >
                                                    {l}
                                                </button>
                                            </li>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            }
                        }}
                    </ul>
                </div>
            </Show>
        </div>
    }
}
