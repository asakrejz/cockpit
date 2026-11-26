//! The part of a ticket's detail that belongs to a GitHub project: labels, open or closed, the
//! values of the project fields, converting a draft – and the dialog for a new issue. Every
//! control is one click that sends one action; the server asks GitHub first and the answer
//! decides (the same rule as for Jira: nothing goes out without a click).

use cockpit_core::{
    FieldDef, FieldInput, FieldKind, FieldValue, Source, TicketStatus,
    project::{DEFAULT_COLUMNS, option_for},
};

use super::*;

/// The project part of the detail of `item`; nothing for any other ticket.
pub(super) fn project_section(store: Store, item: &Item) -> Option<AnyView> {
    let p = item
        .project
        .as_ref()
        .filter(|_| item.source == Source::GitHubProject)?;
    let id = item.id.clone();
    let title = if p.project_title.is_empty() {
        p.project.clone()
    } else {
        p.project_title.clone()
    };
    let fields: Vec<FieldDef> = store.ws.with_untracked(|w| {
        w.sync
            .project(&p.project_id)
            .map(|m| {
                let status = m.status_def().map(|d| d.id.clone());
                m.fields
                    .iter()
                    .filter(|f| Some(&f.id) != status.as_ref())
                    .filter(|f| {
                        f.kind != FieldKind::Other
                            && !f
                                .name
                                .eq_ignore_ascii_case(cockpit_core::project::TITLE_FIELD)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    });
    let issue = p.is_issue();
    let draft = p.draft;
    let closed = p.closed;
    let rows = fields
        .into_iter()
        .map(|def| {
            let current = p.fields.iter().find(|f| f.field_id == def.id).cloned();
            view! { <li><FieldRow store=store item=id.clone() def=def current=current /></li> }
        })
        .collect_view();
    Some(
        view! {
            <List>
                <ListSection header=t!(InboxProjectHeader, title = title) nested=true>
                    {issue.then(|| view! { <li><LabelsRow store=store item=id.clone() /></li> })}
                    {issue
                        .then(|| {
                            let item = id.clone();
                            view! {
                                <li>
                                    <div class="ui-list__row ui-flex-wrap">
                                        <span class="ui-list__title">{t!(InboxProjectState)}</span>
                                        <Status tone=if closed { StatusTone::Gray } else { StatusTone::Green }>
                                            {if closed { t!(InboxProjectStateClosed) } else { t!(InboxProjectStateOpen) }}
                                        </Status>
                                        <Button
                                            size=ControlSize::Small
                                            on_click=move |_| {
                                                let _ = store
                                                    .dispatch(Action::SetIssueClosed {
                                                        item: item.clone(),
                                                        closed: !closed,
                                                    });
                                            }
                                        >
                                            {if closed { t!(InboxProjectReopen) } else { t!(InboxProjectClose) }}
                                        </Button>
                                    </div>
                                </li>
                            }
                        })}
                    {rows}
                    {draft.then(|| view! { <li><DraftRow store=store item=id.clone() /></li> })}
                </ListSection>
            </List>
        }
        .into_any(),
    )
}

/// The labels of the issue: each with a remove button, and a field to add one.
#[component]
fn LabelsRow(store: Store, item: String) -> impl IntoView {
    let id = StoredValue::new(item);
    let new = RwSignal::new(String::new());
    let labels = Memo::new(move |_| {
        store.ws.with(|w| {
            w.item(&id.get_value())
                .map(|i| i.labels.clone())
                .unwrap_or_default()
        })
    });
    let add = move |_| {
        let label = new.get_untracked();
        if label.trim().is_empty() {
            return;
        }
        let _ = store.dispatch(Action::SetLabel {
            item: id.get_value(),
            label,
            on: true,
        });
        new.set(String::new());
    };
    view! {
        <div class="ui-list__row ui-flex-wrap ui-items-start">
            <span class="ui-list__title">{t!(InboxProjectLabels)}</span>
            <div class="ui-vstack ui-gap-8 ck-column">
                <ul class="ck-field__list">
                    <For each=move || labels.get() key=|l| l.clone() let:label>
                        {
                            let name = label.clone();
                            let aria = t!(InboxProjectLabelRemove, label = label.clone());
                            view! {
                                <li>
                                    <Chip>
                                        {label.clone()} " "
                                        <button
                                            type="button"
                                            class="ck-chip-x"
                                            aria-label=aria
                                            on:click=move |_| {
                                                let _ = store
                                                    .dispatch(Action::SetLabel {
                                                        item: id.get_value(),
                                                        label: name.clone(),
                                                        on: false,
                                                    });
                                            }
                                        >
                                            "×"
                                        </button>
                                    </Chip>
                                </li>
                            }
                        }
                    </For>
                    <Show when=move || labels.with(Vec::is_empty)>
                        <li class="t-footnote t-secondary">{t!(InboxProjectLabelNone)}</li>
                    </Show>
                </ul>
                <div class="ui-hstack ui-gap-8">
                    <input
                        class="ui-field__input"
                        type="text"
                        aria-label=t!(InboxProjectLabelPlaceholder)
                        placeholder=t!(InboxProjectLabelPlaceholder)
                        prop:value=move || new.get()
                        on:input=move |ev| new.set(event_target_value(&ev))
                    />
                    <Button size=ControlSize::Small on_click=add>{t!(InboxProjectLabelAdd)}</Button>
                </div>
            </div>
        </div>
    }
}

/// One project field: a list for choices and iterations, a field for numbers, dates and text.
#[component]
fn FieldRow(
    store: Store,
    item: String,
    def: FieldDef,
    current: Option<FieldValue>,
) -> impl IntoView {
    let id = StoredValue::new(item);
    let field_id = StoredValue::new(def.id.clone());
    let name = def.name.clone();
    let send = move |value: Option<FieldInput>| {
        let _ = store.dispatch(Action::SetProjectField {
            item: id.get_value(),
            field_id: field_id.get_value(),
            value,
        });
    };
    let control = match def.kind {
        FieldKind::SingleSelect | FieldKind::Iteration => {
            let iteration = def.kind == FieldKind::Iteration;
            let options: Vec<(String, String)> =
                std::iter::once((String::new(), t!(InboxProjectFieldNone)))
                    .chain(def.options.iter().map(|o| (o.id.clone(), o.name.clone())))
                    .collect();
            let value = current.and_then(|c| c.option_id).unwrap_or_default();
            view! {
                <Select
                    options=options
                    value=value
                    aria_label=name.clone()
                    compact=true
                    on_change=move |picked: String| {
                        send(match (picked.is_empty(), iteration) {
                            (true, _) => None,
                            (false, true) => Some(FieldInput::Iteration(picked)),
                            (false, false) => Some(FieldInput::Select(picked)),
                        })
                    }
                />
            }
            .into_any()
        }
        kind => {
            let text = RwSignal::new(current.map(|c| c.text).unwrap_or_default());
            let input_type = match kind {
                FieldKind::Number => "number",
                FieldKind::Date => "date",
                _ => "text",
            };
            let save = move |_| {
                let t = text.get_untracked();
                send(if t.trim().is_empty() {
                    None
                } else {
                    Some(match kind {
                        FieldKind::Number => FieldInput::Number(t),
                        FieldKind::Date => FieldInput::Date(t),
                        _ => FieldInput::Text(t),
                    })
                });
            };
            view! {
                <div class="ui-hstack ui-gap-8">
                    <input
                        class="ui-field__input"
                        type=input_type
                        step=(kind == FieldKind::Number).then_some("any")
                        aria-label=name.clone()
                        title=t!(InboxProjectFieldHint)
                        prop:value=move || text.get()
                        on:input=move |ev| text.set(event_target_value(&ev))
                    />
                    <Button size=ControlSize::Small on_click=save>{t!(InboxProjectFieldSave)}</Button>
                </div>
            }
            .into_any()
        }
    };
    view! {
        <div class="ui-list__row ui-flex-wrap">
            <span class="ui-list__title">{def.name.clone()}</span>
            {control}
        </div>
    }
}

/// A draft issue: pick the repo and convert it into an issue.
#[component]
fn DraftRow(store: Store, item: String) -> impl IntoView {
    let id = StoredValue::new(item);
    let repo = RwSignal::new(
        store
            .ws
            .with_untracked(|w| w.item_repos().into_iter().next().unwrap_or_default()),
    );
    let convert = move |_| {
        let repo = repo.get_untracked();
        if repo.trim().is_empty() {
            return;
        }
        let _ = store.dispatch(Action::ConvertDraft {
            item: id.get_value(),
            repo: repo.trim().to_string(),
            created: None,
        });
    };
    view! {
        <div class="ui-vstack ui-gap-8 ui-py-8">
            <span class="ui-list__title">{t!(InboxProjectDraftTitle)}</span>
            <p class="t-footnote t-secondary">{t!(InboxProjectDraftText)}</p>
            <TextField
                label=t!(InboxProjectDraftRepo)
                value=repo
                placeholder="acme/app"
                hint=t!(InboxProjectNewRepoHint)
                autocomplete="off"
            />
            <div>
                <Button
                    size=ControlSize::Small
                    disabled=Signal::derive(move || repo.with(|r| r.trim().is_empty()))
                    on_click=convert
                >
                    {t!(InboxProjectDraftConvert)}
                </Button>
            </div>
        </div>
    }
}

/// The columns a new issue can start in: the four statuses the project has a column for.
fn statuses_of(store: Store, project_id: &str) -> Vec<(String, String)> {
    let options = store.ws.with_untracked(|w| {
        w.sync
            .project(project_id)
            .and_then(|m| m.status_def().map(|d| d.options.clone()))
            .unwrap_or_default()
    });
    DEFAULT_COLUMNS
        .iter()
        .map(|(_, s)| *s)
        .filter(|s| option_for(&options, *s).is_some())
        .map(|s| (s.id().to_string(), labels::ticket_status(s)))
        .collect()
}

/// The button "New issue" and its dialog. Shown only when a project is known.
#[component]
pub(super) fn NewIssue(store: Store) -> impl IntoView {
    let open = RwSignal::new(false);
    let projects = Memo::new(move |_| {
        store.ws.with(|w| {
            w.sync
                .projects
                .iter()
                .map(|m| (m.id.clone(), m.title.clone()))
                .collect::<Vec<_>>()
        })
    });
    let project = RwSignal::new(String::new());
    let repo = RwSignal::new(String::new());
    let title = RwSignal::new(String::new());
    let body = RwSignal::new(String::new());
    let status = RwSignal::new(String::new());
    Effect::new(move |_| {
        if open.get() {
            if project.get_untracked().is_empty()
                && let Some((id, _)) = projects.get_untracked().first()
            {
                project.set(id.clone());
            }
            if repo.get_untracked().is_empty() {
                repo.set(
                    store
                        .ws
                        .with_untracked(|w| w.item_repos().into_iter().next().unwrap_or_default()),
                );
            }
        }
    });
    let columns = Memo::new(move |_| statuses_of(store, &project.get()));
    let create = move |_| {
        let (t, r) = (title.get_untracked(), repo.get_untracked());
        if t.trim().is_empty() || r.trim().is_empty() || project.get_untracked().is_empty() {
            return;
        }
        let _ = store.dispatch(Action::CreateIssue {
            project: project.get_untracked(),
            repo: r.trim().to_string(),
            title: t,
            body: body.get_untracked(),
            status: TicketStatus::from_id(&status.get_untracked()),
            created: None,
        });
        title.set(String::new());
        body.set(String::new());
        open.set(false);
    };
    view! {
        <Show when=move || projects.with(|p| !p.is_empty())>
            <Button size=ControlSize::Small on_click=move |_| open.set(true)>
                {t!(InboxProjectNewIssue)}
            </Button>
        </Show>
        <Dialog
            open=open
            title=t!(InboxProjectNewTitle)
            footer=move || {
                view! {
                    <Button on_click=move |_| open.set(false)>{t!(InboxProjectNewCancel)}</Button>
                    <Button
                        variant=ButtonStyle::BorderedProminent
                        disabled=Signal::derive(move || {
                            title.with(|t| t.trim().is_empty()) || repo.with(|r| r.trim().is_empty())
                        })
                        on_click=create
                    >
                        {t!(InboxProjectNewCreate)}
                    </Button>
                }
            }
        >
            <div class="ui-vstack ui-gap-12">
                <p class="t-footnote t-secondary">{t!(InboxProjectNewHint)}</p>
                <Select
                    options=Signal::derive(move || projects.get())
                    value=project
                    label=t!(InboxProjectNewProject)
                    on_change=move |v: String| project.set(v)
                />
                <TextField
                    label=t!(InboxProjectNewRepo)
                    value=repo
                    placeholder="acme/app"
                    hint=t!(InboxProjectNewRepoHint)
                    autocomplete="off"
                />
                <TextField label=t!(InboxProjectNewName) value=title />
                <Textarea value=body label=t!(InboxProjectNewBody) />
                <Select
                    options=Signal::derive(move || {
                        std::iter::once((String::new(), t!(InboxProjectFieldNone)))
                            .chain(columns.get())
                            .collect::<Vec<_>>()
                    })
                    value=status
                    label=t!(InboxProjectNewStatus)
                    on_change=move |v: String| status.set(v)
                />
            </div>
        </Dialog>
    }
}
