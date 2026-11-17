use super::*;

/// Choice of the Jira project. The list comes from Jira (with names), plus the
/// projects of the tickets that are already there. Without projects it stays
/// away. The choice is an action: the server syncs right afterwards.
pub(super) fn project_picker(store: Store) -> impl IntoView {
    let remote = RwSignal::new(Vec::<cockpit_core::api::JiraProject>::new());
    leptos::task::spawn_local(async move {
        match server::fetch_jira_projects().await {
            Ok(r) => remote.set(r.projects),
            // If the server is not reachable, the notice above already says so.
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

/// A switch for a rule. The state lies in the workspace and only changes by action;
/// the switch mirrors it.
pub(super) fn rule_switch(store: Store, rule: Rule) -> impl IntoView {
    let on = RwSignal::new(store.ws.with_untracked(|w| w.rules.enabled(rule)));
    // Workspace → switch (e.g. after resetting the demo).
    Effect::new(move |_| {
        let enabled = store.ws.with(|w| w.rules.enabled(rule));
        if on.get_untracked() != enabled {
            on.set(enabled);
        }
    });
    // Switch → workspace. The first run changes nothing.
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

/// The switch "Status follows the pull request". As with the rules the state lies in the
/// workspace and only changes by action.
pub(super) fn status_follow_switch(store: Store) -> impl IntoView {
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

/// Is the list narrowed by search or source?
pub(super) fn is_narrowed(query: &str, source: SourceFilter, filter: &InboxFilter) -> bool {
    !query.trim().is_empty() || source != SourceFilter::All || !filter.is_default()
}

/// The selections of the filter bar, one per definition in `cockpit_core::FILTERS`. A chosen
/// value that no longer occurs in the data (e.g. after a sync) stays in the list so that the
/// display matches the filter. The empty value is the choice „all“ where a filter has one.
pub(super) fn filter_bar(store: Store, by: RwSignal<InboxFilter>) -> impl IntoView {
    FILTERS
        .iter()
        .map(|def| filter_choice(store, by, def))
        .collect_view()
}

fn filter_choice(
    store: Store,
    by: RwSignal<InboxFilter>,
    def: &'static FilterDef,
) -> impl IntoView {
    let value = Signal::derive(move || by.with(|f| f.get(def.id)));
    let options = Signal::derive(move || {
        let mut list: Vec<(String, String)> = match def.choices {
            Choices::Fixed(fixed) => fixed
                .iter()
                .map(|(v, l)| (v.to_string(), l.to_string()))
                .collect(),
            Choices::FromData => store.ws.with(|w| w.filter_choices(def.id)),
        };
        let current = value.get();
        if !current.is_empty() && !list.iter().any(|(v, _)| *v == current) {
            list.push((current.clone(), current));
        }
        match def.all_label {
            Some(all) => std::iter::once((String::new(), all.to_string()))
                .chain(list)
                .collect(),
            None => list,
        }
    });
    // Filters of tickets only make no sense while there are no tickets.
    let shown = move || {
        def.applies != Applies::Tickets || store.ws.with(|w| w.items.iter().any(Item::is_ticket))
    };
    view! {
        <Show when=shown>
            <Select
                options=options
                value=value
                on_change=move |v: String| {
                    by.update(|f| {
                        f.set(def.id, &v);
                    })
                }
                aria_label=def.label
                compact=true
            />
        </Show>
    }
}
