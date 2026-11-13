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

pub(super) fn some_if_set(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

/// A choice of the filter bar. The empty value means "all", `all` is its label.
/// A chosen value that no longer occurs (e.g. after a sync) stays in the
/// list so that the display matches the filter.
pub(super) fn filter_select(
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
