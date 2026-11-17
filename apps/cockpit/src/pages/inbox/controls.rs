use super::*;

/// The Jira projects to choose from: the list comes from Jira (with names), plus the projects
/// of the tickets that are already there. Fetched once when the dialog is built.
pub(super) fn project_options(store: Store) -> Signal<Vec<(String, String)>> {
    let remote = RwSignal::new(Vec::<cockpit_core::api::JiraProject>::new());
    leptos::task::spawn_local(async move {
        match server::fetch_jira_projects().await {
            Ok(r) => remote.set(r.projects),
            // If the server is not reachable, the notice above already says so.
            Err(CallError::Unreachable) => {}
            Err(e) => store.notify(format!("Jira-Projekte nicht geladen: {}", e.message())),
        }
    });
    Signal::derive(move || {
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
        std::iter::once((String::new(), "Alle Projekte".to_string()))
            .chain(list)
            .collect()
    })
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

/// Is the list narrowed by search or filters?
pub(super) fn is_narrowed(query: &str, filter: &InboxFilter) -> bool {
    !query.trim().is_empty() || !filter.is_default()
}
