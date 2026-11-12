use super::*;

/// Auswahl des Jira-Projekts. Die Liste kommt von Jira (mit Namen), dazu die
/// Projekte der Tickets, die schon da sind. Ohne Projekte bleibt sie
/// weg. Die Wahl ist eine Action: Der Server gleicht danach gleich ab.
pub(super) fn project_picker(store: Store) -> impl IntoView {
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

/// Ein Schalter für eine Regel. Der Zustand liegt im Workspace und ändert sich nur per Action;
/// der Schalter spiegelt ihn.
pub(super) fn rule_switch(store: Store, rule: Rule) -> impl IntoView {
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

/// Ist die Liste durch Suche oder Quelle eingeschränkt?
pub(super) fn is_narrowed(query: &str, source: SourceFilter, filter: &InboxFilter) -> bool {
    !query.trim().is_empty() || source != SourceFilter::All || !filter.is_default()
}

pub(super) fn some_if_set(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

/// Eine Auswahl der Filterleiste. Der leere Wert heißt „alle“, `all` ist seine Beschriftung.
/// Ein gewählter Wert, der nicht mehr vorkommt (etwa nach einem Abgleich), bleibt in der
/// Liste, damit die Anzeige zum Filter passt.
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
