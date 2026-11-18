use super::*;

/// The detail of the chosen entry.
pub(super) fn detail(
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
        .map(|pr| Route::Review(Some(pr.key())).hash());

    // Tickets show their status for switching, pull requests the state of the checks.
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

    // The most important action: commission the agent – or, if there is already a pull request, review.
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

    // Edit ticket: title and description, assignee, priority.
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
        // "Nothing changes" is no reason to stay in edit mode.
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

    // Link: at the ticket the pull request (release or link by hand), at the pull request the ticket.
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
                        .filter(|r| r.merged.is_none() && w.ticket_of_pr(r.key()).is_none())
                        .map(|r| (r.key(), r.title.clone()))
                        .collect::<Vec<_>>()
                });
                open.into_iter()
                    .map(|(number, title)| {
                        let label = format!("Mit {number} verknüpfen");
                        view! {
                            <Button
                                size=ControlSize::Small
                                on_click=move |_| {
                                    let _ = store.dispatch(Action::LinkPr {
                                        item: id.get_value(),
                                        pr: number.clone(),
                                    });
                                }
                            >
                                {label}
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
                    rich=c.rich.clone().unwrap_or_default()
                    own=c.own
                />
            }
        })
        .collect_view();

    // Snooze: four options; if the entry is already snoozed, it shows for how long.
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

    let display_key = item.display_key();
    view! {
        <PaneHeader>
            <div class="ui-hstack" style="--spacing: 10px">
                <Badge variant=BadgeVariant::Neutral>{source}</Badge>
                <span class="t-subheadline t-mono t-secondary">{display_key}</span>
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
            <div hidden=move || edit.on.get() && is_ticket>
                {match item.description_rich.clone() {
                    Some(doc) => view! { <RichText doc=doc /> }.into_any(),
                    None => view! { <p class="t-body">{item.description.clone()}</p> }.into_any(),
                }}
            </div>

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
